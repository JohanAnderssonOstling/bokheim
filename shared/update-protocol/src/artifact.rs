//! Streaming size and SHA-256 verification for release artifacts.

use crate::{decode_hex, Artifact};
use ring::digest::{Context, SHA256};
use std::io::{Read, Write};

pub fn copy_verified(mut input: impl Read, output: &mut impl Write, artifact: &Artifact) -> Result<(), String> {
    let mut digest = Context::new(&SHA256);
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer).map_err(|e| format!("cannot read update: {e}"))?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).ok_or("update size overflow")?;
        if total > artifact.bytes {
            return Err("update exceeds published size".into());
        }
        digest.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(|e| format!("cannot save update: {e}"))?;
    }
    if total != artifact.bytes {
        return Err("update is shorter than published size".into());
    }
    if digest.finish().as_ref() != decode_hex::<32>(&artifact.sha256)? {
        return Err("update failed SHA-256 verification".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_oversized_and_corrupt_artifacts() {
        let artifact = Artifact { url: "https://downloads.example.org/file".into(), bytes: 3, sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into() };
        let mut output = Vec::new();
        copy_verified(&b"abc"[..], &mut output, &artifact).unwrap();
        assert_eq!(output, b"abc");
        for bytes in [&b"ab"[..], &b"abcd"[..], &b"xyz"[..]] {
            assert!(copy_verified(bytes, &mut Vec::new(), &artifact).is_err());
        }
    }
}
