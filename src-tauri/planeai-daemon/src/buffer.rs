use std::collections::VecDeque;

pub struct RingBuffer {
    buf: VecDeque<u8>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.buf.len() == self.capacity {
                self.buf.pop_front();
            }
            self.buf.push_back(b);
        }
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
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
}
