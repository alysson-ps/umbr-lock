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

    pub fn pop_char(&mut self) -> bool {
        if self.bytes.is_empty() {
            return false;
        }

        while let Some(byte) = self.bytes.pop() {
            if (byte & 0b1100_0000) != 0b1000_0000 {
                break;
            }
        }

        true
    }

    pub fn len(&self) -> usize {
        std::str::from_utf8(&self.bytes)
            .map(|s| s.chars().count())
            .unwrap_or(0)
    }

    pub fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.bytes).to_string()
    }
}
