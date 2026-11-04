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
    let mut stream = Stream::new()?;
    stream.push(samples)?;
    stream.finish(0..samples.len())
}

/// Encodes while the user is still talking (M1 plan step 5): every complete 20 ms frame becomes an Opus
/// packet as soon as it is pushed, so only the last partial frame is left when recording stops. The Ogg
/// file is built at the end from the packets that cover the part the voice gate keeps.
pub struct Stream {
    enc: Encoder,
    pre_skip: u64,
    /// Samples pushed so far.
    len: usize,
    /// The tail that does not fill a frame yet.
    pending: Vec<f32>,
    packets: Vec<Vec<u8>>,
}

impl Stream {
    pub fn new() -> Result<Self, EncodeError> {
        let mut enc = Encoder::new(RATE as u32, Channels::Mono, Application::Voip)?;
        enc.set_bitrate(Bitrate::Bits(BITRATE))?;
        enc.set_complexity(COMPLEXITY)?;
        let pre_skip = enc.get_lookahead()? as u64 * (GRANULE_RATE / RATE as u64);
        Ok(Self {
            enc,
            pre_skip,
            len: 0,
            pending: Vec::with_capacity(FRAME),
            packets: Vec::new(),
        })
    }

    /// Mono samples at [`RATE`]; any length.
    pub fn push(&mut self, mut samples: &[f32]) -> Result<(), EncodeError> {
        self.len += samples.len();
        while !samples.is_empty() {
            let take = (FRAME - self.pending.len()).min(samples.len());
            self.pending.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.pending.len() == FRAME {
                self.encode_pending()?;
            }
        }
        Ok(())
    }

    /// Samples pushed so far.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn encode_pending(&mut self) -> Result<(), EncodeError> {
        self.pending.resize(FRAME, 0.0);
        let mut packet = [0u8; MAX_PACKET];
        let n = self.enc.encode_float(&self.pending, &mut packet)?;
        self.packets.push(packet[..n].to_vec());
        self.pending.clear();
        Ok(())
    }

    /// The Ogg/Opus file for `keep` (sample indexes, as [`crate::vad::Detection::keep`]). It starts at the
    /// frame that holds `keep.start`: a decoder starting mid-stream needs a few ms to settle, which falls in
    /// the voice gate's padding. `0..len()` gives the whole clip. The packets stay, so it can be called again
    /// (the trimmed clip for the engine, then the whole recording for the history).
    pub fn finish(&mut self, keep: std::ops::Range<usize>) -> Result<Encoded, EncodeError> {
        let end = keep.end.min(self.len);
        if !self.pending.is_empty() || self.packets.is_empty() {
            self.encode_pending()?;
        }
        let first = (keep.start / FRAME).min(self.packets.len() - 1);
        let last = end.div_ceil(FRAME).clamp(first + 1, self.packets.len());
        let to_granule = GRANULE_RATE / RATE as u64;
        let kept = end.saturating_sub(first * FRAME);

        let mut w = PacketWriter::new(Vec::with_capacity((last - first) * 48 + 256));
        w.write_packet(
            opus_head(self.pre_skip as u16),
            SERIAL,
            PacketWriteEndInfo::EndPage,
            0,
        )?;
        w.write_packet(opus_tags(), SERIAL, PacketWriteEndInfo::EndPage, 0)?;
        let count = last - first;
        for (i, packet) in self.packets[first..last].iter().enumerate() {
            let done = i + 1 == count;
            let granule = self.pre_skip
                + if done {
                    kept as u64
                } else {
                    ((i + 1) * FRAME) as u64
                } * to_granule;
            let info = if done {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            w.write_packet(packet.clone(), SERIAL, info, granule)?;
        }
        Ok(Encoded {
            bytes: w.into_inner(),
            mime: MIME,
            ext: EXT,
            audio_ms: (kept * 1000 / RATE) as u32,
        })
    }
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

    #[test]
    fn pushing_in_odd_chunks_gives_the_same_file_as_one_go() {
        let samples = tone(1.3, 180.0);
        let whole = opus(&samples).unwrap();
        let mut s = Stream::new().unwrap();
        for chunk in samples.chunks(441) {
            s.push(chunk).unwrap();
        }
        assert_eq!(s.len(), samples.len());
        assert_eq!(s.finish(0..samples.len()).unwrap().bytes, whole.bytes);
    }

    #[test]
    fn finish_can_be_called_again_for_the_whole_clip() {
        let samples = tone(1.3, 180.0);
        let whole = opus(&samples).unwrap();
        let mut s = Stream::new().unwrap();
        s.push(&samples).unwrap();
        let part = s.finish(5 * FRAME..20 * FRAME).unwrap();
        assert!(part.bytes.len() < whole.bytes.len());
        assert_eq!(s.finish(0..s.len()).unwrap().bytes, whole.bytes);
    }

    #[test]
    fn finish_keeps_only_the_frames_that_cover_the_range() {
        let samples = tone(3.0, 250.0);
        let mut s = Stream::new().unwrap();
        s.push(&samples).unwrap();
        let keep = 10 * FRAME..60 * FRAME + 7;
        let e = s.finish(keep.clone()).unwrap();
        let (headers, packets, granule, out) = decode(&e.bytes);
        let pre_skip = u16::from_le_bytes([headers[0][10], headers[0][11]]) as u64;
        assert_eq!(packets, 51);
        assert_eq!(granule, pre_skip + (keep.len() as u64) * 3);
        assert_eq!(e.audio_ms, (keep.len() * 1000 / RATE) as u32);
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let (a, b) = (rms(&samples), rms(&out[FRAME * 5..]));
        assert!(
            (b / a - 1.0).abs() < 0.2,
            "starting mid-stream still decodes: rms in {a}, out {b}"
        );
    }
}
