use std::collections::VecDeque;

pub struct RingBuffer {
    buf: VecDeque<u8>,
    capacity: usize,
    /// Monotonic counter of total bytes ever written.
    /// Used to compute cursors for incremental reads.
    total_written: u64,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
            total_written: 0,
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.buf.len() == self.capacity {
                self.buf.pop_front();
            }
            self.buf.push_back(b);
        }
        self.total_written += bytes.len() as u64;
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Return the current write offset (total bytes ever written).
    /// This is the cursor value that points to "right after the last byte".
    pub fn write_offset(&self) -> u64 {
        self.total_written
    }

    /// Return the earliest offset still available in the buffer.
    /// Anything before this has been evicted from the ring.
    pub fn start_offset(&self) -> u64 {
        self.total_written - self.buf.len() as u64
    }

    /// Read bytes written since `after_offset`, up to `max_bytes` (0 = unlimited).
    /// Returns (bytes, truncated) where truncated=true means data before the
    /// requested offset has been evicted.
    pub fn read_after(&self, after_offset: u64, max_bytes: usize) -> (Vec<u8>, bool) {
        let start = self.start_offset();
        let end = self.total_written;

        if after_offset > end {
            // Cursor is in the future — return empty, not truncated
            return (Vec::new(), false);
        }

        let truncated = after_offset < start;
        let effective_start = if truncated { start } else { after_offset };

        // Position within our buffer: offset from start_offset
        let buf_start = (effective_start - start) as usize;
        let available = self.buf.len() - buf_start;

        let take = if max_bytes > 0 {
            available.min(max_bytes)
        } else {
            available
        };

        let bytes: Vec<u8> = self.buf.iter().skip(buf_start).take(take).copied().collect();
        (bytes, truncated)
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_under_capacity() {
        let mut rb = RingBuffer::new(10);
        rb.write(b"hello");
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.snapshot(), b"hello");
    }

    #[test]
    fn write_over_capacity_drops_oldest() {
        let mut rb = RingBuffer::new(5);
        rb.write(b"abcde");
        rb.write(b"fg");
        assert_eq!(rb.len(), 5);
        assert_eq!(rb.snapshot(), b"cdefg");
    }

    #[test]
    fn snapshot_returns_correct_order() {
        let mut rb = RingBuffer::new(4);
        rb.write(b"abcd");
        rb.write(b"ef");
        assert_eq!(rb.snapshot(), b"cdef");
    }

    #[test]
    fn empty_buffer_returns_empty_vec() {
        let rb = RingBuffer::new(10);
        assert_eq!(rb.snapshot(), Vec::<u8>::new());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn write_offset_tracks_total_bytes() {
        let mut rb = RingBuffer::new(10);
        assert_eq!(rb.write_offset(), 0);
        rb.write(b"hello");
        assert_eq!(rb.write_offset(), 5);
        rb.write(b"world");
        assert_eq!(rb.write_offset(), 10);
    }

    #[test]
    fn start_offset_reflects_eviction() {
        let mut rb = RingBuffer::new(5);
        rb.write(b"abcde");
        assert_eq!(rb.start_offset(), 0);
        rb.write(b"fg");
        // 7 total written, 5 in buffer → start = 2
        assert_eq!(rb.start_offset(), 2);
        assert_eq!(rb.write_offset(), 7);
    }

    #[test]
    fn read_after_returns_new_bytes() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"hello");
        let cursor = rb.write_offset(); // 5
        rb.write(b" world");
        let (bytes, truncated) = rb.read_after(cursor, 0);
        assert_eq!(bytes, b" world");
        assert!(!truncated);
    }

    #[test]
    fn read_after_with_eviction_returns_truncated() {
        let mut rb = RingBuffer::new(5);
        rb.write(b"abcde");
        let cursor = rb.write_offset(); // 5
        rb.write(b"fghij"); // evicts a-e, buffer is now f-j, total=10
        rb.write(b"k"); // evicts f, buffer is now g-k, total=11

        // cursor=5, start_offset=6 → truncated
        let (bytes, truncated) = rb.read_after(cursor, 0);
        assert!(truncated);
        // Returns everything from start_offset (6) to end (11): ghijk
        assert_eq!(bytes, b"ghijk");
    }

    #[test]
    fn read_after_future_cursor_returns_empty() {
        let mut rb = RingBuffer::new(10);
        rb.write(b"hello");
        let (bytes, truncated) = rb.read_after(999, 0);
        assert!(bytes.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn read_after_with_max_bytes_caps_output() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"hello world");
        let (bytes, truncated) = rb.read_after(0, 5);
        assert_eq!(bytes, b"hello");
        assert!(!truncated);
    }

    #[test]
    fn read_after_zero_offset_returns_all() {
        let mut rb = RingBuffer::new(100);
        rb.write(b"hello");
        let (bytes, truncated) = rb.read_after(0, 0);
        assert_eq!(bytes, b"hello");
        assert!(!truncated);
    }
}
