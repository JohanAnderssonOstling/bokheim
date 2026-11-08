//! Exact duration for contiguous Layer III streams without packet allocation.

use std::io;

use symphonia::core::io::{MediaSourceStream, ReadBytes};

pub(super) struct FrameHeader {
    pub bytes: u32,
    pub samples: u32,
    pub sample_rate: u32,
    pub side_info_len: usize,
}

impl FrameHeader {
    pub fn parse(word: u32) -> Option<Self> {
        let version = (word >> 19) & 3;
        let bitrate = ((word >> 12) & 15) as usize;
        let rate = ((word >> 10) & 3) as usize;
        if word & 0xffe0_0000 != 0xffe0_0000 || version == 1 || (word >> 17) & 3 != 1 || matches!(bitrate, 0 | 15) || rate == 3 {
            return None;
        }
        let mpeg1 = version == 3;
        let bitrates = if mpeg1 { [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0] } else { [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0] };
        let sample_rate = [44100, 48000, 32000][rate]
            / if mpeg1 {
                1
            } else if version == 2 {
                2
            } else {
                4
            };
        let bytes = (if mpeg1 { 144000 } else { 72000 }) * bitrates[bitrate] / sample_rate + ((word >> 9) & 1);
        let mono = (word >> 6) & 3 == 3;
        let side_info_len = match (mpeg1, mono) {
            (true, true) => 17,
            (true, false) => 32,
            (false, true) => 9,
            (false, false) => 17,
        };
        Some(Self { bytes, samples: if mpeg1 { 1152 } else { 576 }, sample_rate, side_info_len })
    }
}

struct FrameLayout {
    format: u32,
    header: FrameHeader,
    lengths: [u32; 16],
}

impl FrameLayout {
    const FORMAT_MASK: u32 = 0xfffe_0c00; // Sync, MPEG version/layer, sample rate.

    fn new(word: u32) -> Option<Self> {
        let header = FrameHeader::parse(word)?;
        // Version and sample rate are fixed for this fast path. Calculate all
        // bitrate lengths once, then account for the per-frame padding bit.
        let lengths = std::array::from_fn(|bitrate| FrameHeader::parse((word & !0xf200) | ((bitrate as u32) << 12)).map_or(0, |header| header.bytes));
        Some(Self { format: word & Self::FORMAT_MASK, header, lengths })
    }

    fn frame_size(&self, word: u32) -> Option<u32> {
        let bytes = self.lengths[((word >> 12) & 15) as usize];
        (word & Self::FORMAT_MASK == self.format && bytes != 0).then_some(bytes + ((word >> 9) & 1))
    }
}

/// The caller has already probed the stream, so the cursor is at the first
/// audio packet, after any initial Xing/Info/VBRI frame. Unexpected tag frames,
/// resynchronization and changing stream parameters use the normal demuxer.
pub(super) fn sample_count(reader: &mut MediaSourceStream, length: u64, sample_rate: u32) -> io::Result<Option<u64>> {
    let mut samples = 0u64;
    let mut layout = None;
    while reader.pos() < length {
        let remaining = length - reader.pos();
        if remaining < 4 {
            // No complete header can remain, matching the demuxer's EOF.
            return Ok(Some(samples));
        }
        let bytes = reader.read_quad_bytes()?;
        let word = u32::from_be_bytes(bytes);
        if layout.is_none() {
            layout = FrameLayout::new(word);
        }
        let Some(frame_bytes) = layout.as_ref().and_then(|layout| layout.frame_size(word)) else {
            // A small trailer without any MPEG sync word cannot add duration.
            // This includes ordinary ID3v1 tags and zero padding. Suspicious
            // trailers remain the demuxer's responsibility.
            if remaining > 4096 {
                return Ok(None);
            }
            let mut trailer = [0; 4096];
            trailer[..4].copy_from_slice(&bytes);
            reader.read_buf_exact(&mut trailer[4..remaining as usize])?;
            return Ok((!trailer[..remaining as usize].windows(2).any(|pair| pair[0] == 0xff && pair[1] & 0xe0 == 0xe0)).then_some(samples));
        };
        let layout = layout.as_ref().unwrap();
        if layout.header.sample_rate != sample_rate || u64::from(frame_bytes) > remaining {
            return Ok(None);
        }
        // Both tag types require zeroed side information. Most audio frames
        // fail that check in their first four bytes, so avoid copying the rest
        // of their prefixes as well as their compressed payloads.
        let first = reader.read_quad_bytes()?;
        let mut consumed = 8;
        if first == [0; 4] {
            let mut prefix = [0; 36];
            let prefix_len = (frame_bytes as usize - 4).min(prefix.len());
            reader.read_buf_exact(&mut prefix[4..prefix_len])?;
            consumed = 4 + prefix_len as u64;
            let mpeg1 = layout.header.samples == 1152;
            let mono = (word >> 6) & 3 == 3;
            let offset = match (mpeg1, mono) {
                (true, true) => 17,
                (true, false) => 32,
                (false, true) => 9,
                (false, false) => 17,
            };
            if frame_bytes as usize >= 4 + offset + 8 && matches!(&prefix[offset..offset + 4], b"Xing" | b"Info") && prefix[..offset].iter().all(|byte| *byte == 0) {
                return Ok(None);
            }
            if frame_bytes >= 62 && &prefix[32..36] == b"VBRI" && prefix[..32].iter().all(|byte| *byte == 0) {
                return Ok(None);
            }
        }
        reader.ignore_bytes(u64::from(frame_bytes) - consumed)?;
        samples = samples.checked_add(u64::from(layout.header.samples)).ok_or_else(|| io::Error::other("MP3 duration overflow"))?;
    }
    Ok(Some(samples))
}
