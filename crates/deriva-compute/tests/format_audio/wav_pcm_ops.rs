use bytes::Bytes;
use deriva_compute::builtins_format_audio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- wav_to_raw_pcm (5 tests) ----
#[test]
fn wav_to_raw_pcm_basic() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = WavToRawPcmFn.execute(vec![wav], &p(&[])).unwrap();
    // Output starts with JSON header line
    let nl = out.iter().position(|&b| b == b'\n').unwrap();
    let header: serde_json::Value = serde_json::from_slice(&out[..nl]).unwrap();
    assert_eq!(header["sample_rate"], 44100);
    assert_eq!(header["channels"], 1);
    assert_eq!(header["bits_per_sample"], 16);
    // Remaining bytes are raw PCM
    assert!(out.len() > nl + 1);
}

#[test]
fn wav_to_raw_pcm_stereo() {
    let wav = make_wav(440.0, 100, 44100, 2);
    let out = WavToRawPcmFn.execute(vec![wav], &p(&[])).unwrap();
    let nl = out.iter().position(|&b| b == b'\n').unwrap();
    let header: serde_json::Value = serde_json::from_slice(&out[..nl]).unwrap();
    assert_eq!(header["channels"], 2);
}

#[test]
fn wav_to_raw_pcm_not_wav() {
    assert!(WavToRawPcmFn.execute(vec![Bytes::from_static(b"not a wav")], &p(&[])).is_err());
}

#[test]
fn wav_to_raw_pcm_no_input() {
    assert!(WavToRawPcmFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn wav_to_raw_pcm_id() {
    assert_eq!(WavToRawPcmFn.id().name, "wav_to_raw_pcm");
}

// ---- raw_pcm_to_wav (5 tests) ----
#[test]
fn raw_pcm_to_wav_basic() {
    // Create raw PCM: 100 samples of silence
    let pcm: Vec<u8> = vec![0u8; 200]; // 100 samples × 2 bytes
    let out = RawPcmToWavFn.execute(
        vec![Bytes::from(pcm)],
        &p(&[("sample_rate", "44100"), ("channels", "1"), ("bits_per_sample", "16")]),
    ).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
    assert_eq!(&out[8..12], b"WAVE");
}

#[test]
fn raw_pcm_to_wav_roundtrip() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let pcm_out = WavToRawPcmFn.execute(vec![wav], &p(&[])).unwrap();
    let nl = pcm_out.iter().position(|&b| b == b'\n').unwrap();
    let raw_pcm = Bytes::copy_from_slice(&pcm_out[nl + 1..]);
    let wav2 = RawPcmToWavFn.execute(
        vec![raw_pcm],
        &p(&[("sample_rate", "44100"), ("channels", "1"), ("bits_per_sample", "16")]),
    ).unwrap();
    assert_eq!(&wav2[0..4], b"RIFF");
    // Should be valid WAV that can be parsed back
    let meta = AudioMetadataFn.execute(vec![wav2], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&meta).unwrap();
    assert_eq!(v["sample_rate"], 44100);
}

#[test]
fn raw_pcm_to_wav_stereo() {
    let pcm = vec![0u8; 400]; // 100 samples × 2 channels × 2 bytes
    let out = RawPcmToWavFn.execute(
        vec![Bytes::from(pcm)],
        &p(&[("sample_rate", "22050"), ("channels", "2"), ("bits_per_sample", "16")]),
    ).unwrap();
    assert_eq!(u16::from_le_bytes([out[22], out[23]]), 2); // channels field
}

#[test]
fn raw_pcm_to_wav_no_input() {
    assert!(RawPcmToWavFn.execute(vec![], &p(&[("sample_rate", "44100"), ("channels", "1"), ("bits_per_sample", "16")])).is_err());
}

#[test]
fn raw_pcm_to_wav_id() {
    assert_eq!(RawPcmToWavFn.id().name, "raw_pcm_to_wav");
}
