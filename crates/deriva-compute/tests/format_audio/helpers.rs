use bytes::Bytes;
use deriva_core::address::Value;
use std::collections::BTreeMap;

pub fn p(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// Generate a valid WAV file with a sine wave tone.
/// `freq_hz`: tone frequency, `duration_ms`: length, `sr`: sample rate, `ch`: channels
pub fn make_wav(freq_hz: f64, duration_ms: u64, sr: u32, ch: u16) -> Bytes {
    let num_samples = (sr as u64 * duration_ms / 1000) as usize;
    let bps: u16 = 16;
    let data_len = (num_samples * ch as usize * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&ch.to_le_bytes());
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&(sr * ch as u32 * bps as u32 / 8).to_le_bytes());
    buf.extend_from_slice(&(ch * bps / 8).to_le_bytes());
    buf.extend_from_slice(&bps.to_le_bytes());
    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..num_samples {
        let t = i as f64 / sr as f64;
        let sample = (f64::sin(2.0 * std::f64::consts::PI * freq_hz * t) * 16000.0) as i16;
        for _ in 0..ch {
            buf.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Bytes::from(buf)
}

/// Generate a WAV with silence (all zeros).
pub fn make_silent_wav(duration_ms: u64, sr: u32) -> Bytes {
    make_wav(0.0, duration_ms, sr, 1)
}

/// Generate a WAV with a silence gap in the middle:
/// tone for `tone_ms`, silence for `silence_ms`, tone for `tone_ms`.
pub fn make_wav_with_silence(tone_ms: u64, silence_ms: u64, sr: u32) -> Bytes {
    let ch: u16 = 1;
    let bps: u16 = 16;
    let tone_samples = (sr as u64 * tone_ms / 1000) as usize;
    let silence_samples = (sr as u64 * silence_ms / 1000) as usize;
    let total_samples = tone_samples * 2 + silence_samples;
    let data_len = (total_samples * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&ch.to_le_bytes());
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&(sr * ch as u32 * bps as u32 / 8).to_le_bytes());
    buf.extend_from_slice(&(ch * bps / 8).to_le_bytes());
    buf.extend_from_slice(&bps.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    let freq = 440.0;
    // First tone
    for i in 0..tone_samples {
        let t = i as f64 / sr as f64;
        let s = (f64::sin(2.0 * std::f64::consts::PI * freq * t) * 16000.0) as i16;
        buf.extend_from_slice(&s.to_le_bytes());
    }
    // Silence
    for _ in 0..silence_samples {
        buf.extend_from_slice(&0i16.to_le_bytes());
    }
    // Second tone
    for i in 0..tone_samples {
        let t = i as f64 / sr as f64;
        let s = (f64::sin(2.0 * std::f64::consts::PI * freq * t) * 16000.0) as i16;
        buf.extend_from_slice(&s.to_le_bytes());
    }
    Bytes::from(buf)
}
