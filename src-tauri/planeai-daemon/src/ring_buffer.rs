/// Fixed-capacity ring buffer for scrollback.
pub struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
    write_pos: usize,
    len: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            cap: capacity,
            write_pos: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        for &b in data {
            self.buf[self.write_pos] = b;
            self.write_pos = (self.write_pos + 1) % self.cap;
        }
        self.len = (self.len + data.len()).min(self.cap);
    }

    /// Returns the buffered content in order.
    pub fn contents(&self) -> Vec<u8> {
        if self.len < self.cap {
            self.buf[..self.len].to_vec()
        } else {
            let start = self.write_pos;
            let mut out = Vec::with_capacity(self.cap);
            out.extend_from_slice(&self.buf[start..]);
            out.extend_from_slice(&self.buf[..start]);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_within_capacity() {
        let mut rb = RingBuffer::new(16);
        rb.push(b"hello");
        assert_eq!(rb.contents(), b"hello");
    }

    #[test]
    fn push_wraps_around() {
        let mut rb = RingBuffer::new(8);
        rb.push(b"abcdefgh"); // fills exactly
        rb.push(b"XY"); // overwrites first 2
        assert_eq!(rb.contents(), b"cdefghXY");
    }

    #[test]
    fn empty_buffer() {
        let rb = RingBuffer::new(64);
        assert_eq!(rb.contents(), b"");
    }
}
