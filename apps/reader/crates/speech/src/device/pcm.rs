//! Validate and downmix provider audio before it enters the playback queue.
use crate::SpeechError;
use super::Audio;
pub(crate) const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn decode(bytes: &[u8], format: u16, channels: u16, sample_rate: u32) -> Result<Audio, SpeechError> {
    let width = match format {
        8 => 1,
        16 => 2,
        32 => 4,
        _ => return Err("The local voice returned an unsupported audio format.".into()),
    };
    let frame = width * usize::from(channels);
    if channels == 0 || channels > 8 || !(8_000..=192_000).contains(&sample_rate) || (bytes.is_empty() || bytes.len() > MAX_AUDIO_BYTES) || frame == 0 || !bytes.len().is_multiple_of(frame) {
        return Err("The local voice returned invalid or oversized audio.".into());
    }
    let mut samples = Vec::with_capacity(bytes.len() / frame);
    for source in bytes.chunks_exact(frame) {
        let mut value = 0.0;
        for sample in source.chunks_exact(width) {
            let next = match format {
                8 => (f32::from(sample[0]) - 128.0) / 128.0,
                16 => f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0,
                32 => f32::from_le_bytes(sample.try_into().expect("four-byte float")),
                _ => unreachable!(),
            };
            if !next.is_finite() {
                return Err("The local voice returned invalid audio samples.".into());
            }
            value += next.clamp(-1.0, 1.0) / f32::from(channels);
        }
        samples.push(value);
    }
    Ok(Audio { samples, sample_rate, last: true })
}

#[cfg(any(test, target_os = "windows"))]
pub(crate) fn wave(bytes: &[u8]) -> Result<Audio, SpeechError> {
    let invalid = || SpeechError::from("The local voice returned an invalid WAV audio stream.");
    if bytes.len() < 12 || bytes.len() > MAX_AUDIO_BYTES || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(invalid());
    }
    let mut offset = 12usize;
    let mut format = None;
    let mut data = None;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[offset..offset + 4];
        let count = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().map_err(|_| invalid())?) as usize;
        offset += 8;
        let end = offset.checked_add(count).filter(|end| *end <= bytes.len()).ok_or_else(invalid)?;
        let chunk = &bytes[offset..end];
        if id == b"fmt " && chunk.len() >= 16 {
            let encoding = u16::from_le_bytes([chunk[0], chunk[1]]);
            let channels = u16::from_le_bytes([chunk[2], chunk[3]]);
            let rate = u32::from_le_bytes(chunk[4..8].try_into().map_err(|_| invalid())?);
            let bits = u16::from_le_bytes([chunk[14], chunk[15]]);
            if !matches!((encoding, bits), (1, 8) | (1, 16) | (3, 32)) {
                return Err("The local voice returned an unsupported WAV audio format.".into());
            }
            format = Some((bits, channels, rate));
        } else if id == b"data" {
            data = Some(chunk);
        }
        offset = end.checked_add(count % 2).ok_or_else(invalid)?;
    }
    let (bits, channels, rate) = format.ok_or_else(invalid)?;
    decode(data.ok_or_else(invalid)?, bits, channels, rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_pcm_is_downmixed_and_bad_frames_or_float_samples_are_rejected() {
        let audio = decode(&[0, 64, 0, 192], 16, 2, 22050).unwrap();
        assert_eq!(audio.samples, [0.0]);
        assert!(decode(&[0], 16, 1, 22050).is_err());
        assert!(decode(&f32::NAN.to_le_bytes(), 32, 1, 22050).is_err());
        assert!(decode(&[0, 0], 16, 0, 22050).is_err());
    }
    #[test]
    fn windows_wave_stream_accepts_metadata_and_downmixes_pcm() {
        let mut bytes = b"RIFF\0\0\0\0WAVEJUNK\x01\0\0\0x\0fmt \x10\0\0\0".to_vec();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&22050u32.to_le_bytes());
        bytes.extend_from_slice(&88200u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data\x04\0\0\0\0\x40\0\xc0");
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let audio = wave(&bytes).unwrap();
        assert_eq!(audio.sample_rate, 22050);
        assert_eq!(audio.samples, [0.0]);
        assert!(audio.last);
    }
    #[test]
    fn truncated_and_unbounded_wave_chunks_fail_without_panicking() {
        assert!(wave(b"RIFF0000WAVEdata\xff\xff\xff\xff").is_err());
        for length in 0..64 {
            assert!(wave(&vec![0; length]).is_err());
        }
    }
}
