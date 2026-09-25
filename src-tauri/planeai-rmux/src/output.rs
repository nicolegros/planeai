//! Bounded output buffering with explicit gap accounting.
//!
//! Probe 7 (ADR-0012) established that rmux applies no backpressure to the child
//! process: a subscriber that stops reading has its output **dropped** by the
//! daemon, reported after the fact as a lag notice. PlaneAI's `SessionBackend`
//! contract, by contrast, expects `pause()` to withhold delivery to the frontend
//! without losing the child's output.
//!
//! The reconciliation is this buffer. The reader task drains rmux continuously —
//! never pausing the transport — and parks bytes here. Pause and resume then act
//! on delivery to the frontend, not on the transport. If a paused consumer
//! outlasts the buffer, the loss becomes PlaneAI's own bounded, reported drop
//! rather than an unbounded daemon-side one.

/// Why a discontinuity appeared in a session's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapCause {
    /// The daemon dropped output because this subscriber fell behind.
    Transport { missed_events: u64 },
    /// This buffer discarded the oldest bytes to stay within its cap.
    LocalOverflow { dropped_bytes: u64 },
    /// A recovery keyframe could not carry all retained scrollback, so rows above
    /// the reconstructed screen are gone.
    HistoryTruncated { rows: u64 },
}

/// A coalesced gap notice awaiting delivery to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub cause: GapCause,
}

impl Gap {
    /// The notice to splice into the terminal stream where output is missing.
    ///
    /// A gap is rendered inline rather than raised as a UI event because the
    /// terminal is the only place that can show *where* the discontinuity fell.
    /// ADR-0012 requires a drop never be silent, and a log line the user cannot
    /// see does not satisfy that: the scrollback would simply appear to jump.
    ///
    /// Written as its own line in dim SGR so it cannot be mistaken for the
    /// child's own output, and CR-prefixed because the child may have left the
    /// cursor mid-row.
    pub fn notice(&self) -> Vec<u8> {
        let detail = match self.cause {
            GapCause::Transport { missed_events } => format!(
                "{missed_events} output event(s) dropped by the rmux daemon — this session fell behind"
            ),
            GapCause::LocalOverflow { dropped_bytes } => format!(
                "{dropped_bytes} byte(s) of output discarded — buffered output exceeded its cap"
            ),
            GapCause::HistoryTruncated { rows } => format!(
                "{rows} row(s) of scrollback above this point could not be recovered"
            ),
        };
        format!("\r\n\x1b[2m[planeai] output gap: {detail}\x1b[0m\r\n").into_bytes()
    }
}

/// A byte buffer that never grows past `capacity`, dropping the oldest bytes and
/// recording a gap when it would.
#[derive(Debug)]
pub struct OutputBuffer {
    capacity: usize,
    bytes: Vec<u8>,
    pending_gap: Option<Gap>,
    dropped_bytes: u64,
    missed_events: u64,
}

impl OutputBuffer {
    /// A cap large enough for a burst of scrollback while bounding memory per
    /// session. Matches the existing daemon adapter's 1 MiB ceiling.
    pub const DEFAULT_CAPACITY: usize = 1_048_576;

    /// Capacity kept across flushes so the hot path does not reallocate.
    ///
    /// The reader must not allocate per chunk: probe 8 (ADR-0012) showed a
    /// consumer that falls behind has its output dropped by the daemon, so the
    /// cost of a `malloc` per chunk is paid in lost bytes, not just cycles.
    /// `take()` hands its allocation to the frontend, so without a reserve the
    /// next `push` starts from nothing and grows by repeated reallocation.
    ///
    /// Sized for a burst of ordinary chunks rather than the full cap: reserving
    /// `DEFAULT_CAPACITY` per session would cost a megabyte for every idle
    /// terminal, and a session that genuinely needs more grows once and keeps it.
    const RETAINED_CAPACITY: usize = 64 * 1024;

    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: Vec::with_capacity(capacity.min(Self::RETAINED_CAPACITY)),
            pending_gap: None,
            dropped_bytes: 0,
            missed_events: 0,
        }
    }

    pub fn with_default_capacity() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Append output from the daemon, discarding the oldest bytes if the cap
    /// would be exceeded.
    pub fn push(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        // A chunk larger than the whole buffer keeps only its tail: the newest
        // bytes are what the terminal needs to render.
        if chunk.len() >= self.capacity {
            let keep_from = chunk.len() - self.capacity;
            self.record_local_drop((self.bytes.len() + keep_from) as u64);
            self.bytes.clear();
            self.bytes.extend_from_slice(&chunk[keep_from..]);
            return;
        }

        let overflow = (self.bytes.len() + chunk.len()).saturating_sub(self.capacity);
        if overflow > 0 {
            self.bytes.drain(..overflow);
            self.record_local_drop(overflow as u64);
        }
        self.bytes.extend_from_slice(chunk);
    }

    /// Record a daemon-side drop reported by a lag notice.
    pub fn record_transport_gap(&mut self, missed_events: u64) {
        self.missed_events = self.missed_events.saturating_add(missed_events);
        // A transport gap outranks a local one: it means output never reached us
        // at all, which is the more severe statement to make to the frontend.
        self.pending_gap = Some(Gap {
            cause: GapCause::Transport {
                missed_events: self.missed_events,
            },
        });
    }

    /// Record scrollback a recovery keyframe could not carry.
    pub fn record_history_shortfall(&mut self, rows: u64) {
        if rows == 0 {
            return;
        }
        self.pending_gap = Some(Gap {
            cause: GapCause::HistoryTruncated { rows },
        });
    }

    /// Discard everything undelivered and start again from a recovery keyframe.
    ///
    /// A rebase is an atomic replacement of the screen, and the SDK is explicit
    /// that state must never be stitched across epochs. Bytes still queued belong
    /// to the previous epoch, so delivering them after the keyframe would render
    /// output the keyframe has already accounted for.
    pub fn replace_with_keyframe(&mut self, keyframe: &[u8]) {
        self.bytes.clear();
        self.push(keyframe);
    }

    fn record_local_drop(&mut self, dropped: u64) {
        self.dropped_bytes = self.dropped_bytes.saturating_add(dropped);
        if !matches!(
            self.pending_gap.map(|gap| gap.cause),
            Some(GapCause::Transport { .. })
        ) {
            self.pending_gap = Some(Gap {
                cause: GapCause::LocalOverflow {
                    dropped_bytes: self.dropped_bytes,
                },
            });
        }
    }

    /// Remove and return the buffered bytes.
    ///
    /// Replaces the buffer with a fresh one that already has
    /// [`Self::RETAINED_CAPACITY`], rather than the empty `Vec` `mem::take` would
    /// leave behind: the flusher calls this on every delivery, so a zero-capacity
    /// replacement puts an allocation back on the hot path for each one.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::replace(
            &mut self.bytes,
            Vec::with_capacity(self.capacity.min(Self::RETAINED_CAPACITY)),
        )
    }

    /// Remove and return a pending gap notice, if one has accumulated.
    pub fn take_gap(&mut self) -> Option<Gap> {
        self.pending_gap.take()
    }

    /// Total bytes this buffer has discarded for its own capacity limit.
    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    /// Room allocated for buffered bytes, whether used or not.
    ///
    /// Exposed so tests can assert the hot path stays allocation-free; callers
    /// have no reason to care.
    #[doc(hidden)]
    pub fn allocated_capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Total events the daemon reported as dropped for this subscriber.
    pub fn missed_events(&self) -> u64 {
        self.missed_events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flush_leaves_capacity_behind_so_the_hot_path_does_not_reallocate() {
        // The reader must not allocate per chunk: a consumer that falls behind has
        // its output dropped by the daemon (ADR-0012 probe 8), so an allocation on
        // this path is paid in lost bytes. `take()` hands its buffer to the
        // frontend, so it has to leave a pre-sized one in its place.
        let mut buffer = OutputBuffer::with_default_capacity();
        let reserved = buffer.allocated_capacity();
        assert!(
            reserved > 0,
            "a new buffer should start with room to push into"
        );

        buffer.push(b"some output");
        let taken = buffer.take();
        assert_eq!(taken, b"some output");

        // The replacement must be ready to accept the next chunk without growing.
        assert_eq!(
            buffer.allocated_capacity(),
            reserved,
            "take() left a smaller buffer, so the next push reallocates"
        );
        buffer.push(b"more output");
        assert_eq!(
            buffer.allocated_capacity(),
            reserved,
            "pushing within the retained capacity must not reallocate"
        );
    }

    #[test]
    fn a_tiny_cap_does_not_reserve_more_than_it_will_ever_hold() {
        // The reserve is bounded by the cap so a small buffer stays small.
        let buffer = OutputBuffer::new(8);
        assert_eq!(buffer.allocated_capacity(), 8);
    }

    #[test]
    fn a_rebase_replaces_undelivered_bytes_rather_than_appending_to_them() {
        // Epochs must never be stitched together: bytes still queued belong to the
        // screen the keyframe supersedes, so delivering both would render output
        // the keyframe already accounts for.
        let mut buffer = OutputBuffer::with_default_capacity();
        buffer.push(b"stale output from the previous epoch");

        buffer.replace_with_keyframe(b"KEYFRAME");

        assert_eq!(buffer.take(), b"KEYFRAME");
    }

    #[test]
    fn a_truncated_keyframe_reports_the_rows_it_could_not_carry() {
        let mut buffer = OutputBuffer::with_default_capacity();
        buffer.record_history_shortfall(120);

        let gap = buffer.take_gap().expect("a shortfall is a gap");
        assert_eq!(gap.cause, GapCause::HistoryTruncated { rows: 120 });
        let notice = String::from_utf8(gap.notice()).unwrap();
        assert!(notice.contains("120 row(s) of scrollback"), "{notice}");
    }

    #[test]
    fn a_keyframe_that_carried_everything_is_not_a_gap() {
        let mut buffer = OutputBuffer::with_default_capacity();
        buffer.record_history_shortfall(0);
        assert!(buffer.take_gap().is_none());
    }

    #[test]
    fn a_transport_gap_notice_names_the_daemon_as_the_source() {
        // The user has to be able to tell "the daemon dropped output because I fell
        // behind" from "PlaneAI discarded output to stay within its cap" — the two
        // have different fixes.
        let gap = Gap {
            cause: GapCause::Transport { missed_events: 12 },
        };
        let notice = String::from_utf8(gap.notice()).unwrap();

        assert!(
            notice.contains("12 output event(s) dropped by the rmux daemon"),
            "{notice}"
        );
        assert!(notice.contains("[planeai] output gap"), "{notice}");
        // Its own line, in dim SGR, so it cannot be mistaken for child output.
        assert!(notice.starts_with("\r\n\x1b[2m"), "{notice:?}");
        assert!(notice.ends_with("\x1b[0m\r\n"), "{notice:?}");
    }

    #[test]
    fn a_local_overflow_notice_reports_bytes_rather_than_events() {
        let gap = Gap {
            cause: GapCause::LocalOverflow {
                dropped_bytes: 2048,
            },
        };
        let notice = String::from_utf8(gap.notice()).unwrap();

        assert!(
            notice.contains("2048 byte(s) of output discarded"),
            "{notice}"
        );
        assert!(!notice.contains("rmux daemon"), "{notice}");
    }

    #[test]
    fn accumulates_below_capacity_without_reporting_a_gap() {
        let mut buffer = OutputBuffer::new(16);
        buffer.push(b"abc");
        buffer.push(b"def");

        assert_eq!(buffer.len(), 6);
        assert_eq!(buffer.take(), b"abcdef");
        assert_eq!(buffer.take_gap(), None);
        assert!(buffer.is_empty());
    }

    #[test]
    fn take_clears_the_buffer_so_bytes_are_delivered_once() {
        let mut buffer = OutputBuffer::new(16);
        buffer.push(b"once");

        assert_eq!(buffer.take(), b"once");
        assert_eq!(buffer.take(), b"");
    }

    #[test]
    fn overflow_keeps_the_newest_bytes_and_reports_a_local_gap() {
        let mut buffer = OutputBuffer::new(8);
        buffer.push(b"12345678");
        buffer.push(b"9ab");

        // The oldest three bytes are gone; the newest are what a terminal needs.
        assert_eq!(buffer.take(), b"456789ab");
        assert_eq!(
            buffer.take_gap(),
            Some(Gap {
                cause: GapCause::LocalOverflow { dropped_bytes: 3 }
            })
        );
        assert_eq!(buffer.dropped_bytes(), 3);
    }

    #[test]
    fn a_chunk_larger_than_capacity_keeps_only_its_tail() {
        let mut buffer = OutputBuffer::new(4);
        buffer.push(b"0123456789");

        assert_eq!(buffer.take(), b"6789");
        assert!(matches!(
            buffer.take_gap(),
            Some(Gap {
                cause: GapCause::LocalOverflow { dropped_bytes: 6 }
            })
        ));
    }

    #[test]
    fn a_gap_is_reported_once_then_cleared() {
        let mut buffer = OutputBuffer::new(2);
        buffer.push(b"abcd");

        assert!(buffer.take_gap().is_some());
        assert_eq!(buffer.take_gap(), None);
    }

    #[test]
    fn transport_gaps_accumulate_missed_events() {
        let mut buffer = OutputBuffer::new(64);
        buffer.record_transport_gap(10);
        buffer.record_transport_gap(5);

        assert_eq!(
            buffer.take_gap(),
            Some(Gap {
                cause: GapCause::Transport { missed_events: 15 }
            })
        );
        assert_eq!(buffer.missed_events(), 15);
    }

    #[test]
    fn a_transport_gap_outranks_a_local_overflow() {
        let mut buffer = OutputBuffer::new(2);
        buffer.record_transport_gap(7);
        buffer.push(b"abcd");

        // Local overflow must not mask the more severe statement that output
        // never reached PlaneAI at all.
        assert_eq!(
            buffer.take_gap(),
            Some(Gap {
                cause: GapCause::Transport { missed_events: 7 }
            })
        );
        assert_eq!(buffer.dropped_bytes(), 2);
    }

    #[test]
    fn pushing_nothing_is_not_a_gap() {
        let mut buffer = OutputBuffer::new(1);
        buffer.push(b"");

        assert!(buffer.is_empty());
        assert_eq!(buffer.take_gap(), None);
    }
}
