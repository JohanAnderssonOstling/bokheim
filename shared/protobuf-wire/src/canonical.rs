//! Compare Prost's canonical output directly with its input. Payload slices are
//! compared in place; only small primitive encodings use the scratch buffer.
use prost::bytes::{buf::UninitSlice, Buf, BufMut};
use prost::Message;

pub(crate) fn matches(message: &impl Message, expected: &[u8]) -> bool {
    if message.encoded_len() != expected.len() {
        return false;
    }
    let mut output = Comparison { expected, position: 0, equal: true, scratch: [0; 16] };
    message.encode_raw(&mut output);
    output.equal && output.position == expected.len()
}

struct Comparison<'a> {
    expected: &'a [u8],
    position: usize,
    equal: bool,
    scratch: [u8; 16],
}

// SAFETY: chunk_mut exposes initialized writable storage bounded by
// remaining_mut. advance_mut checks that bound before comparing and advancing.
// Storage is never resized or freed while a chunk is borrowed.
unsafe impl BufMut for Comparison<'_> {
    fn remaining_mut(&self) -> usize {
        self.expected.len() - self.position
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        let length = self.remaining_mut().min(self.scratch.len());
        UninitSlice::new(&mut self.scratch[..length])
    }

    unsafe fn advance_mut(&mut self, count: usize) {
        assert!(count <= self.remaining_mut().min(self.scratch.len()));
        self.equal &= self.expected[self.position..self.position + count] == self.scratch[..count];
        self.position += count;
    }

    fn put_slice(&mut self, bytes: &[u8]) {
        assert!(bytes.len() <= self.remaining_mut());
        self.equal &= self.expected[self.position..self.position + bytes.len()] == *bytes;
        self.position += bytes.len();
    }

    fn put_u8(&mut self, byte: u8) {
        self.put_slice(&[byte]);
    }

    fn put<T: Buf>(&mut self, mut bytes: T)
    where
        Self: Sized,
    {
        assert!(bytes.remaining() <= self.remaining_mut());
        while bytes.has_remaining() {
            let chunk = bytes.chunk();
            let length = chunk.len();
            self.put_slice(chunk);
            bytes.advance(length);
        }
    }
}
