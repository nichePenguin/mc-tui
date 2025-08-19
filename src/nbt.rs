#[derive(Debug)]
pub struct NbtData {
    bytes: Box<[u8]>
}

impl NbtData {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        NbtData {
            bytes: Box::from(bytes)
        }
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn to_bytes(&self) -> Box<[u8]> {
        Box::from(self.bytes.clone())
    }
}

