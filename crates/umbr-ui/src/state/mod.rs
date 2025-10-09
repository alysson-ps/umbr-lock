#[derive(Debug)]
pub struct PasswordBuffer {
    pub bytes: Vec<u8>,
}

impl PasswordBuffer {
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(128),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0; 4];
        let s = c.encode_utf8(&mut buf);
        self.bytes.extend_from_slice(s.as_bytes());
    }
}