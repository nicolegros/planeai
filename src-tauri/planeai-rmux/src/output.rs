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
}

/// A coalesced gap notice awaiting delivery to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub cause: GapCause,
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

    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: Vec::new(),
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
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }

    /// Remove and return a pending gap notice, if one has accumulated.
    pub fn take_gap(&mut self) -> Option<Gap> {
        self.pending_gap.take()
    }

    /// Total bytes this buffer has discarded for its own capacity limit.
    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
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
