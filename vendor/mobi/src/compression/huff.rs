use std::convert::TryInto;
use std::io;

use thiserror::Error;

type HuffmanResult<T> = Result<T, HuffmanError>;

#[derive(Debug, Error)]
pub enum HuffmanError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error("code len out of bounds")]
    CodeLenOutOfBounds,
    #[error("bad termination code")]
    BadTerm,
    #[error("expected HUFF magic header")]
    InvalidHuffHeader,
    #[error("expected CDIC magic header")]
    InvalidCDICHeader,
    #[error("the provided index to huffman dictionary was out of bounds")]
    InvalidDictionaryIndex,
    #[error("cyclic or excessively nested HUFF/CDIC dictionary")]
    DictionaryRecursion,
    #[error("HUFF/CDIC dictionary exceeds its byte limit")]
    DictionaryLimit,
}

type HuffmanDictionary = Vec<Option<(Vec<u8>, bool)>>;
type CodeDictionary = [(u8, bool, u32); 256];

/// A book's HUFF/CDIC tables and phrase cache, independent of its source.
/// Load once, then decode individual text records with `decode_into`.
#[derive(Debug)]
pub struct HuffmanDecoder {
    dictionary: HuffmanDictionary,
    code_dict: CodeDictionary,
    min_codes: [u32; 33],
    max_codes: [u32; 33],
    dictionary_bytes: usize,
    expansion_bytes: usize,
    dictionary_limit: usize,
}

fn word(bytes: &[u8], offset: usize) -> HuffmanResult<u32> {
    let bytes = bytes.get(offset..).and_then(|tail| tail.get(..4)).ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "truncated HUFF/CDIC table"))?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

impl HuffmanDecoder {
    /// `records` contains the HUFF record followed by all CDIC records.
    /// `dictionary_limit` bounds phrase bytes and dictionary slots, including
    /// cached expansions. Compressed input records need not remain alive.
    pub fn new(records: &[&[u8]], dictionary_limit: usize) -> HuffmanResult<Self> {
        let mut decoder = Self { dictionary: Vec::new(), code_dict: [(0, false, 0); 256], min_codes: [0; 33], max_codes: [u32::MAX; 33], dictionary_bytes: 0, expansion_bytes: 0, dictionary_limit };
        let huff = records.first().ok_or(HuffmanError::InvalidHuffHeader)?;
        decoder.load_huff(huff)?;
        let mut phrase_count = None;
        for cdic in &records[1..] {
            decoder.load_cdic(cdic, &mut phrase_count)?;
        }
        if phrase_count != Some(decoder.dictionary.len()) || decoder.dictionary.is_empty() {
            return Err(HuffmanError::InvalidCDICHeader);
        }
        Ok(decoder)
    }

    fn load_huff(&mut self, huff: &[u8]) -> HuffmanResult<()> {
        if huff.get(..4) != Some(b"HUFF") || word(huff, 4)? != 0x18 {
            return Err(HuffmanError::InvalidHuffHeader);
        }
        let cache = word(huff, 8)? as usize;
        let base = word(huff, 12)? as usize;
        let cache = huff.get(cache..).and_then(|bytes| bytes.get(..256 * 4)).ok_or(HuffmanError::InvalidHuffHeader)?;
        let base = huff.get(base..).and_then(|bytes| bytes.get(..32 * 8)).ok_or(HuffmanError::InvalidHuffHeader)?;
        for (index, code) in self.code_dict.iter_mut().enumerate() {
            let value = word(cache, index * 4)?;
            let length = (value & 0x1f) as u8;
            let terminal = value & 0x80 != 0;
            if length == 0 {
                return Err(HuffmanError::CodeLenOutOfBounds);
            }
            if length <= 8 && !terminal {
                return Err(HuffmanError::BadTerm);
            }
            // Use a wider integer for the inclusive upper bound (which can
            // legitimately be 2^32 - 1).
            let max = ((u64::from(value >> 8) + 1) << (32 - length)) - 1;
            if max > u64::from(u32::MAX) {
                return Err(HuffmanError::BadTerm);
            }
            *code = (length, terminal, max as u32);
        }
        for length in 1..=32 {
            let min = u64::from(word(base, (length - 1) * 8)?) << (32 - length);
            let max = ((u64::from(word(base, (length - 1) * 8 + 4)?) + 1) << (32 - length)) - 1;
            if min > u64::from(u32::MAX) || max > u64::from(u32::MAX) || min > max {
                return Err(HuffmanError::BadTerm);
            }
            self.min_codes[length] = min as u32;
            self.max_codes[length] = max as u32;
        }
        Ok(())
    }

    fn load_cdic(&mut self, cdic: &[u8], phrase_count: &mut Option<usize>) -> HuffmanResult<()> {
        if cdic.get(..4) != Some(b"CDIC") || word(cdic, 4)? != 0x10 {
            return Err(HuffmanError::InvalidCDICHeader);
        }
        let phrases = word(cdic, 8)? as usize;
        let bits = word(cdic, 12)?;
        if phrase_count.is_some_and(|count| count != phrases) || bits > 16 {
            return Err(HuffmanError::InvalidCDICHeader);
        }
        *phrase_count = Some(phrases);
        let remaining = phrases.checked_sub(self.dictionary.len()).ok_or(HuffmanError::InvalidCDICHeader)?;
        let count = (1usize << bits).min(remaining);
        let offsets = cdic.get(16..16 + count * 2).ok_or(HuffmanError::InvalidCDICHeader)?;
        for offset in offsets.chunks_exact(2) {
            let position = 16 + usize::from(u16::from_be_bytes(offset.try_into().unwrap()));
            let header = cdic.get(position..position + 2).ok_or(HuffmanError::InvalidCDICHeader)?;
            let length = u16::from_be_bytes(header.try_into().unwrap());
            let bytes = cdic.get(position + 2..position + 2 + usize::from(length & 0x7fff)).ok_or(HuffmanError::InvalidCDICHeader)?;
            let cost = bytes.len() + std::mem::size_of::<Option<(Vec<u8>, bool)>>();
            self.dictionary_bytes = self.dictionary_bytes.checked_add(cost).filter(|&size| size <= self.dictionary_limit).ok_or(HuffmanError::DictionaryLimit)?;
            self.dictionary.push(Some((bytes.to_vec(), length & 0x8000 != 0)));
        }
        Ok(())
    }

    /// Append decompressed bytes until `output.len() == output_limit`.
    /// The limit allows callers to decode bounded frontmatter as well as full
    /// text. Trailing MOBI record data must be removed by the caller first.
    pub fn decode_into(&mut self, data: &[u8], output: &mut Vec<u8>, output_limit: usize) -> HuffmanResult<()> {
        self.unpack_into(data, output, output_limit, 0)
    }

    fn unpack_into(&mut self, data: &[u8], output: &mut Vec<u8>, limit: usize, depth: usize) -> HuffmanResult<()> {
        if depth > 64 {
            return Err(HuffmanError::DictionaryRecursion);
        }
        let bit_count = data.len().checked_mul(8).ok_or(HuffmanError::CodeLenOutOfBounds)?;
        let mut position = 0;
        while position < bit_count && output.len() < limit {
            // A padded 32-bit lookahead works even for short records and the
            // final partial code, without reading past the record boundary.
            let byte = position / 8;
            let shift = position % 8;
            let mut window = [0u8; 8];
            let count = (data.len() - byte).min(5);
            window[..count].copy_from_slice(&data[byte..byte + count]);
            let code = ((u64::from_be_bytes(window) << shift) >> 32) as u32;
            let (length, terminal, mut max) = self.code_dict[(code >> 24) as usize];
            let mut length = usize::from(length);
            if !terminal {
                while length <= 32 && code < self.min_codes[length] {
                    length += 1;
                }
                if length > 32 {
                    return Err(HuffmanError::CodeLenOutOfBounds);
                }
                max = self.max_codes[length];
            }
            if length > bit_count - position {
                break; // trailing padding is not a complete code
            }
            let index = (max.checked_sub(code).ok_or(HuffmanError::BadTerm)? >> (32 - length)) as usize;
            self.expand_phrase(index, depth + 1)?;
            let phrase = &self.dictionary[index].as_ref().ok_or(HuffmanError::DictionaryRecursion)?.0;
            let count = phrase.len().min(limit - output.len());
            if depth > 0 {
                let cost = self.dictionary_bytes.checked_add(self.expansion_bytes).and_then(|cost| cost.checked_add(count)).ok_or(HuffmanError::DictionaryLimit)?;
                if cost > self.dictionary_limit {
                    return Err(HuffmanError::DictionaryLimit);
                }
                self.expansion_bytes += count;
            }
            output.extend_from_slice(&phrase[..count]);
            position += length;
        }
        Ok(())
    }

    fn expand_phrase(&mut self, index: usize, depth: usize) -> HuffmanResult<()> {
        let slot = self.dictionary.get(index).ok_or(HuffmanError::InvalidDictionaryIndex)?;
        let (_, expanded) = slot.as_ref().ok_or(HuffmanError::DictionaryRecursion)?;
        if *expanded {
            return Ok(());
        }
        // Empty the slot while expanding it so a recursive reference becomes
        // an ordinary error. Restore the compressed phrase on failure.
        let (bytes, _) = self.dictionary[index].take().unwrap();
        let mut output = Vec::new();
        let result = self.unpack_into(&bytes, &mut output, usize::MAX, depth);
        self.expansion_bytes -= output.len();
        if let Err(error) = result {
            self.dictionary[index] = Some((bytes, false));
            return Err(error);
        }
        let size = self.dictionary_bytes - bytes.len() + output.len();
        if size > self.dictionary_limit {
            self.dictionary[index] = Some((bytes, false));
            return Err(HuffmanError::DictionaryLimit);
        }
        self.dictionary_bytes = size;
        self.dictionary[index] = Some((output, true));
        Ok(())
    }
}

// Preserve the original whole-book entry point for upstream callers.
pub fn decompress(huffs: &[&[u8]], sections: &[&[u8]]) -> HuffmanResult<Vec<Vec<u8>>> {
    let mut decoder = HuffmanDecoder::new(huffs, 64 * 1024 * 1024)?;
    let mut output = Vec::new();
    let mut remaining = 512 * 1024 * 1024;
    for section in sections {
        let mut bytes = Vec::new();
        decoder.decode_into(section, &mut bytes, remaining)?;
        remaining -= bytes.len();
        output.push(bytes);
    }
    Ok(output)
}
