//! Race SimData decode.
//!
//! `<SimDataBase64>k__BackingField` is base64( zlib/gzip( packed little-endian
//! binary ) ). Binary layout mirrors hakuraku's `RaceDataParser`:
//!
//! - header: `i32 maxLength`, `i32 version`; body starts at `4 + maxLength`.
//! - race struct (16B): `f32 distanceDiffMax, i32 horseNum, i32 horseFrameSize, i32 horseResultSize`.
//! - `i32 PaddingSize1`; skip `4 + PaddingSize1`.
//! - `i32 frameCount`, `i32 frameSize`; then `frameCount` frames of stride `frameSize`.
//!   - frame = `f32 time` + `horseNum` × horseFrame
//!     (`f32 distance, u16 lane, u16 speed, u16 hp, i8 temptation, i8 blockFront`, 12B).
//!
//! Step 1 only summarizes the race; per-frame indexing comes later.

use std::io::Read;

use base64::Engine;

/// High-level facts decoded from a race SimData blob.
///
/// Some fields (stride sizes, `distance_diff_max`) are retained for fidelity and
/// future use even if not all are surfaced yet.
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct RaceSummary {
    pub version: i32,
    pub horse_num: i32,
    pub frame_count: i32,
    pub frame_size: i32,
    pub horse_frame_size: i32,
    pub distance_diff_max: f32,
    /// Max horse distance in the final frame (includes post-finish over-run; NOT
    /// the official course distance).
    pub race_length_m: f32,
}

/// One runner's per-frame state.
///
/// `lane`/`block_front` are decoded for fidelity; not all are surfaced yet.
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub struct RunnerSampleRaw {
    pub distance: f32,
    pub lane: u16,
    pub speed: u16,
    pub hp: u16,
    pub temptation: i8,
    pub block_front: i8,
}

/// One simulation frame: a timestamp and every runner's state at that time.
#[derive(Clone, Debug, Default)]
pub struct FrameData {
    pub time: f32,
    pub runners: Vec<RunnerSampleRaw>,
}

/// Fully decoded race: summary facts plus every frame.
#[derive(Clone, Debug, Default)]
pub struct DecodedRace {
    pub summary: RaceSummary,
    pub frames: Vec<FrameData>,
}

/// Decode a base64 SimData string into a full [`DecodedRace`] (summary + frames).
pub fn decode_full(base64_str: &str) -> Result<DecodedRace, &'static str> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_str.trim())
        .map_err(|_| "base64 decode failed")?;
    let bytes = inflate(&raw).ok_or("inflate failed")?;
    parse_full(&bytes).ok_or("binary parse failed")
}

/// Inflate zlib, gzip, or raw-deflate payloads (pako.inflate accepts zlib/gzip).
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    out.clear();
    if flate2::read::GzDecoder::new(data).read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    out.clear();
    if flate2::read::DeflateDecoder::new(data).read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Some(out);
    }
    None
}

fn rd_i32(b: &[u8], off: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn rd_f32(b: &[u8], off: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn rd_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn rd_i8(b: &[u8], off: usize) -> Option<i8> {
    Some(*b.get(off)? as i8)
}

fn parse_full(b: &[u8]) -> Option<DecodedRace> {
    let max_length = rd_i32(b, 0)?;
    let version = rd_i32(b, 4)?;
    if max_length < 0 {
        return None;
    }
    let mut off = 4usize.checked_add(max_length as usize)?;

    let distance_diff_max = rd_f32(b, off)?;
    let horse_num = rd_i32(b, off + 4)?;
    let horse_frame_size = rd_i32(b, off + 8)?;
    let _horse_result_size = rd_i32(b, off + 12)?;
    off = off.checked_add(16)?;

    // Sanity bounds — a race has a handful of runners and small fixed strides.
    if !(1..=64).contains(&horse_num) || !(12..=4096).contains(&horse_frame_size) {
        return None;
    }

    let padding1 = rd_i32(b, off)?;
    if padding1 < 0 {
        return None;
    }
    off = off.checked_add(4)?.checked_add(padding1 as usize)?;

    let frame_count = rd_i32(b, off)?;
    let frame_size = rd_i32(b, off + 4)?;
    off = off.checked_add(8)?;
    if !(1..=1_000_000).contains(&frame_count) || !(1..=65_536).contains(&frame_size) {
        return None;
    }

    let horse_num_usize = horse_num as usize;
    let horse_frame_size_usize = horse_frame_size as usize;
    let frame_size_usize = frame_size as usize;
    // Each frame must hold its timestamp plus every runner's stride.
    if 4 + horse_num_usize.checked_mul(horse_frame_size_usize)? > frame_size_usize {
        return None;
    }

    let frames_start = off;
    let frames_bytes = (frame_count as usize).checked_mul(frame_size_usize)?;
    let frames_end = frames_start.checked_add(frames_bytes)?;
    if frames_end > b.len() {
        return None;
    }

    let mut frames = Vec::with_capacity(frame_count as usize);
    let mut race_length_m = 0.0f32;
    for f in 0..frame_count as usize {
        let frame_off = frames_start + f * frame_size_usize;
        let time = rd_f32(b, frame_off)?;
        let mut runners = Vec::with_capacity(horse_num_usize);
        for i in 0..horse_num_usize {
            let h = frame_off + 4 + i * horse_frame_size_usize;
            let sample = RunnerSampleRaw {
                distance: rd_f32(b, h)?,
                lane: rd_u16(b, h + 4)?,
                speed: rd_u16(b, h + 6)?,
                hp: rd_u16(b, h + 8)?,
                temptation: rd_i8(b, h + 10)?,
                block_front: rd_i8(b, h + 11)?,
            };
            if f + 1 == frame_count as usize && sample.distance.is_finite() && sample.distance > race_length_m {
                race_length_m = sample.distance;
            }
            runners.push(sample);
        }
        frames.push(FrameData { time, runners });
    }

    Some(DecodedRace {
        summary: RaceSummary {
            version,
            horse_num,
            frame_count,
            frame_size,
            horse_frame_size,
            distance_diff_max,
            race_length_m,
        },
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::{decode_full, parse_full};

    /// Build a minimal valid blob: empty header, 2 horses, 2 frames.
    fn synthetic() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&4i32.to_le_bytes()); // maxLength = 4 (body starts at 4+4=8)
        b.extend_from_slice(&7i32.to_le_bytes()); // version
                                                  // body @ off=8: race struct
        b.extend_from_slice(&3.5f32.to_le_bytes()); // distanceDiffMax
        b.extend_from_slice(&2i32.to_le_bytes()); // horseNum
        b.extend_from_slice(&12i32.to_le_bytes()); // horseFrameSize
        b.extend_from_slice(&39i32.to_le_bytes()); // horseResultSize
        b.extend_from_slice(&0i32.to_le_bytes()); // PaddingSize1 = 0
        b.extend_from_slice(&2i32.to_le_bytes()); // frameCount
        let frame_size: i32 = 4 + 2 * 12;
        b.extend_from_slice(&frame_size.to_le_bytes()); // frameSize
                                                        // 2 frames
        for f in 0..2 {
            b.extend_from_slice(&(f as f32).to_le_bytes()); // time
            for h in 0..2 {
                let dist = 100.0f32 * (f as f32 + 1.0) + h as f32; // last frame max ~201
                b.extend_from_slice(&dist.to_le_bytes());
                b.extend_from_slice(&0u16.to_le_bytes()); // lane
                b.extend_from_slice(&20u16.to_le_bytes()); // speed
                b.extend_from_slice(&500u16.to_le_bytes()); // hp
                b.push(0); // temptation
                b.push(-1i8 as u8); // blockFront
            }
        }
        b
    }

    #[test]
    fn parses_synthetic_blob() {
        let d = parse_full(&synthetic()).expect("parse");
        let s = d.summary;
        assert_eq!(s.version, 7);
        assert_eq!(s.horse_num, 2);
        assert_eq!(s.frame_count, 2);
        assert_eq!(d.frames.len(), 2);
        assert_eq!(d.frames[1].runners.len(), 2);
        assert!((s.distance_diff_max - 3.5).abs() < 1e-3);
        assert!((s.race_length_m - 201.0).abs() < 1e-3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode_full("not base64 @@@").is_err());
        assert!(parse_full(&[0u8; 8]).is_none());
    }
}
