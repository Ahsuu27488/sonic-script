//! PCM → Ogg/Opus 16 kbps (M1 plan step 3, D-011): the same format as M0's `opus16` (`ffmpeg -c:a libopus
//! -b:a 16k -application voip -f ogg`), which was as accurate as WAV at 1/16 of the size. Written here with
//! libopus + an Ogg writer instead of ffmpeg, so the app needs no external program.
//!
//! The stream follows RFC 7845: an `OpusHead` page, an `OpusTags` page, then one 20 ms packet per Opus frame.
//! Granule positions count 48 kHz samples (always, whatever the input rate) and include the encoder's pre-skip.

use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Encoder};

use crate::vad::{FRAME, RATE};

pub const BITRATE: i32 = 16_000;
/// libopus effort, 0-10; 10 is libopus's (and M0's ffmpeg) default. It encodes ~32x faster than real time on
/// the owner's PC (4 s clip: ~125 ms). 5 is twice as fast but lost 1 point of accuracy and 4 of English
/// kept on the M0 clips (2026-11-03), so the speed has to come from encoding while recording (step 5).
pub const COMPLEXITY: i32 = 10;
pub const MIME: &str = "audio/ogg";
pub const EXT: &str = "ogg";
/// Ogg granule positions for Opus always count 48 kHz samples.
const GRANULE_RATE: u64 = 48_000;
const SERIAL: u32 = 0x5355_4e43; // "SUNC"
/// Far above what a 20 ms packet at 16 kbps needs (40 bytes); libopus's recommended ceiling.
const MAX_PACKET: usize = 4000;

/// An encoded clip, ready to upload.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
    /// Length of the audio that went in.
    pub audio_ms: u32,
}

#[derive(Debug)]
pub struct EncodeError(String);

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encode failed: {}", self.0)
    }
}

impl std::error::Error for EncodeError {}

impl From<opus::Error> for EncodeError {
    fn from(e: opus::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<std::io::Error> for EncodeError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

/// Mono samples in [-1, 1] at [`RATE`] → Ogg/Opus at [`BITRATE`]. The last frame is padded with silence;
/// the end granule position tells decoders to drop the padding.
pub fn opus(samples: &[f32]) -> Result<Encoded, EncodeError> {
    let mut enc = Encoder::new(RATE as u32, Channels::Mono, Application::Voip)?;
    enc.set_bitrate(Bitrate::Bits(BITRATE))?;
    enc.set_complexity(COMPLEXITY)?;
    let to_granule = GRANULE_RATE / RATE as u64;
    let pre_skip = enc.get_lookahead()? as u64 * to_granule;

    let mut w = PacketWriter::new(Vec::with_capacity(samples.len() / 8 + 256));
    w.write_packet(
        opus_head(pre_skip as u16),
        SERIAL,
        PacketWriteEndInfo::EndPage,
        0,
    )?;
    w.write_packet(opus_tags(), SERIAL, PacketWriteEndInfo::EndPage, 0)?;

    let frames = samples.len().div_ceil(FRAME).max(1);
    let end_granule = pre_skip + samples.len() as u64 * to_granule;
    let mut frame = [0f32; FRAME];
    let mut packet = [0u8; MAX_PACKET];
    for i in 0..frames {
        let chunk = &samples[(i * FRAME).min(samples.len())..((i + 1) * FRAME).min(samples.len())];
        frame[..chunk.len()].copy_from_slice(chunk);
        frame[chunk.len()..].fill(0.0);
        let n = enc.encode_float(&frame, &mut packet)?;
        let last = i + 1 == frames;
        let granule = if last {
            end_granule
        } else {
            pre_skip + ((i + 1) * FRAME) as u64 * to_granule
        };
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        w.write_packet(packet[..n].to_vec(), SERIAL, end, granule)?;
    }
    Ok(Encoded {
        bytes: w.into_inner(),
        mime: MIME,
        ext: EXT,
        audio_ms: (samples.len() * 1000 / RATE) as u32,
    })
}

/// RFC 7845 §5.1: identification header, mono, channel mapping family 0.
fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut h = Vec::with_capacity(19);
    h.extend_from_slice(b"OpusHead");
    h.push(1); // version
    h.push(1); // channels
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&(RATE as u32).to_le_bytes()); // input rate, informational
    h.extend_from_slice(&0i16.to_le_bytes()); // output gain
    h.push(0); // mapping family
    h
}

/// RFC 7845 §5.2: comment header with the vendor string and no comments.
fn opus_tags() -> Vec<u8> {
    let vendor = concat!("sonicscript ", env!("CARGO_PKG_VERSION"));
    let mut t = Vec::with_capacity(16 + vendor.len());
    t.extend_from_slice(b"OpusTags");
    t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    t.extend_from_slice(vendor.as_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogg::reading::PacketReader;
    use std::io::Cursor;

    fn tone(seconds: f32, hz: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| 0.3 * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    /// Decodes the stream back and returns (headers, packets, last granule, decoded samples at 16 kHz).
    fn decode(bytes: &[u8]) -> (Vec<Vec<u8>>, usize, u64, Vec<f32>) {
        let mut r = PacketReader::new(Cursor::new(bytes));
        let mut headers = Vec::new();
        let mut packets = 0;
        let mut granule = 0;
        let mut dec = opus::Decoder::new(RATE as u32, Channels::Mono).unwrap();
        let mut out = Vec::new();
        let mut buf = [0f32; FRAME * 6];
        while let Some(p) = r.read_packet().unwrap() {
            if headers.len() < 2 {
                headers.push(p.data);
                continue;
            }
            packets += 1;
            granule = p.absgp_page();
            let n = dec.decode_float(&p.data, &mut buf, false).unwrap();
            out.extend_from_slice(&buf[..n]);
        }
        (headers, packets, granule, out)
    }

    #[test]
    fn stream_has_rfc7845_headers_and_granules() {
        let samples = tone(1.01, 220.0); // not a whole number of frames
        let e = opus(&samples).unwrap();
        assert_eq!(e.mime, "audio/ogg");
        assert_eq!(e.audio_ms, 1010);
        assert_eq!(&e.bytes[..4], b"OggS");
        let (headers, packets, granule, _) = decode(&e.bytes);
        assert_eq!(&headers[0][..8], b"OpusHead");
        assert_eq!(headers[0][9], 1, "mono");
        assert_eq!(&headers[1][..8], b"OpusTags");
        assert_eq!(packets, samples.len().div_ceil(FRAME));
        let pre_skip = u16::from_le_bytes([headers[0][10], headers[0][11]]) as u64;
        assert_eq!(granule, pre_skip + samples.len() as u64 * 3);
    }

    #[test]
    fn about_2_kb_per_second_and_decodes_to_the_same_tone() {
        let samples = tone(4.0, 300.0);
        let e = opus(&samples).unwrap();
        // 16 kbps = 2,000 bytes/s of audio plus Ogg overhead; WAV would be 32,000 bytes/s.
        assert!(
            (6_000..12_000).contains(&e.bytes.len()),
            "{} bytes",
            e.bytes.len()
        );
        let (_, _, _, out) = decode(&e.bytes);
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let (a, b) = (rms(&samples), rms(&out[FRAME * 5..]));
        assert!((b / a - 1.0).abs() < 0.2, "rms in {a}, out {b}");
    }

    #[test]
    fn empty_clip_still_makes_a_valid_stream() {
        let e = opus(&[]).unwrap();
        let (headers, packets, _, _) = decode(&e.bytes);
        assert_eq!(headers.len(), 2);
        assert_eq!(packets, 1);
    }
}
