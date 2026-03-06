use bytes::Bytes;
use deriva_compute::builtins_format_audio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- audio_metadata (5 tests) ----
#[test]
fn audio_metadata_wav() {
    let wav = make_wav(440.0, 500, 44100, 2);
    let out = AudioMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["sample_rate"], 44100);
    assert_eq!(v["channels"], 2);
}

#[test]
fn audio_metadata_mono() {
    let wav = make_wav(440.0, 200, 22050, 1);
    let out = AudioMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["sample_rate"], 22050);
    assert_eq!(v["channels"], 1);
}

#[test]
fn audio_metadata_has_duration() {
    let wav = make_wav(440.0, 1000, 44100, 1);
    let out = AudioMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let dur = v["duration_ms"].as_u64().unwrap();
    assert!(dur >= 900 && dur <= 1100, "duration_ms={dur}");
}

#[test]
fn audio_metadata_invalid() {
    assert!(AudioMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn audio_metadata_id() {
    assert_eq!(AudioMetadataFn.id().name, "audio_metadata");
}

// ---- audio_convert (5 tests) ----
#[test]
fn audio_convert_to_wav() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = AudioConvertFn.execute(vec![wav], &p(&[("format", "wav")])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_convert_default_wav() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = AudioConvertFn.execute(vec![wav], &p(&[])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_convert_unsupported() {
    let wav = make_wav(440.0, 100, 44100, 1);
    assert!(AudioConvertFn.execute(vec![wav], &p(&[("format", "mp3")])).is_err());
}

#[test]
fn audio_convert_no_input() {
    assert!(AudioConvertFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn audio_convert_id() {
    assert_eq!(AudioConvertFn.id().name, "audio_convert");
}

// ---- audio_trim (5 tests) ----
#[test]
fn audio_trim_basic() {
    let wav = make_wav(440.0, 1000, 44100, 1);
    let out = AudioTrimFn.execute(vec![wav], &p(&[("start_ms", "200"), ("end_ms", "800")])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
    // Trimmed WAV should be smaller than original
    let orig = make_wav(440.0, 1000, 44100, 1);
    assert!(out.len() < orig.len());
}

#[test]
fn audio_trim_start_ge_end() {
    let wav = make_wav(440.0, 1000, 44100, 1);
    assert!(AudioTrimFn.execute(vec![wav], &p(&[("start_ms", "500"), ("end_ms", "200")])).is_err());
}

#[test]
fn audio_trim_clamp_beyond_duration() {
    let wav = make_wav(440.0, 500, 44100, 1);
    // end_ms beyond actual duration — should clamp
    let out = AudioTrimFn.execute(vec![wav], &p(&[("start_ms", "0"), ("end_ms", "9999")])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_trim_no_input() {
    assert!(AudioTrimFn.execute(vec![], &p(&[("start_ms", "0"), ("end_ms", "100")])).is_err());
}

#[test]
fn audio_trim_id() {
    assert_eq!(AudioTrimFn.id().name, "audio_trim");
}

// ---- audio_normalize (5 tests) ----
#[test]
fn audio_normalize_basic() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioNormalizeFn.execute(vec![wav], &p(&[("target_db", "-1")])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_normalize_silent() {
    let wav = make_silent_wav(200, 44100);
    let out = AudioNormalizeFn.execute(vec![wav.clone()], &p(&[])).unwrap();
    // Silent audio returned unchanged (same size)
    assert_eq!(out.len(), wav.len());
}

#[test]
fn audio_normalize_default_target() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioNormalizeFn.execute(vec![wav], &p(&[])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_normalize_no_input() {
    assert!(AudioNormalizeFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn audio_normalize_id() {
    assert_eq!(AudioNormalizeFn.id().name, "audio_normalize");
}

// ---- audio_waveform (5 tests) ----
#[test]
fn audio_waveform_produces_png() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioWaveformFn.execute(vec![wav], &p(&[("width", "100"), ("height", "50")])).unwrap();
    // PNG magic bytes
    assert_eq!(&out[0..4], &[0x89, 0x50, 0x4E, 0x47]);
}

#[test]
fn audio_waveform_default_size() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioWaveformFn.execute(vec![wav], &p(&[])).unwrap();
    assert_eq!(&out[0..4], &[0x89, 0x50, 0x4E, 0x47]);
}

#[test]
fn audio_waveform_stereo() {
    let wav = make_wav(440.0, 200, 44100, 2);
    let out = AudioWaveformFn.execute(vec![wav], &p(&[("width", "50"), ("height", "30")])).unwrap();
    assert!(!out.is_empty());
}

#[test]
fn audio_waveform_no_input() {
    assert!(AudioWaveformFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn audio_waveform_id() {
    assert_eq!(AudioWaveformFn.id().name, "audio_waveform");
}

// ---- audio_silence_detect (5 tests) ----
#[test]
fn audio_silence_detect_finds_gap() {
    // 200ms tone, 600ms silence, 200ms tone at 44100Hz
    let wav = make_wav_with_silence(200, 600, 44100);
    let out = AudioSilenceDetectFn.execute(vec![wav], &p(&[("threshold_db", "-30"), ("min_duration_ms", "500")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty(), "should detect silence gap");
}

#[test]
fn audio_silence_detect_no_silence() {
    let wav = make_wav(440.0, 500, 44100, 1);
    let out = AudioSilenceDetectFn.execute(vec![wav], &p(&[("threshold_db", "-40"), ("min_duration_ms", "500")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap().is_empty());
}

#[test]
fn audio_silence_detect_all_silent() {
    let wav = make_silent_wav(1000, 44100);
    let out = AudioSilenceDetectFn.execute(vec![wav], &p(&[("threshold_db", "-40"), ("min_duration_ms", "500")])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(!v.as_array().unwrap().is_empty());
}

#[test]
fn audio_silence_detect_no_input() {
    assert!(AudioSilenceDetectFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn audio_silence_detect_id() {
    assert_eq!(AudioSilenceDetectFn.id().name, "audio_silence_detect");
}

// ---- audio_strip_metadata (5 tests) ----
#[test]
fn audio_strip_metadata_produces_wav() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioStripMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    assert_eq!(&out[0..4], b"RIFF");
}

#[test]
fn audio_strip_metadata_preserves_audio() {
    let wav = make_wav(440.0, 200, 44100, 1);
    let out = AudioStripMetadataFn.execute(vec![wav.clone()], &p(&[])).unwrap();
    // Output should be similar size (WAV → decode → WAV)
    let ratio = out.len() as f64 / wav.len() as f64;
    assert!(ratio > 0.9 && ratio < 1.1, "ratio={ratio}");
}

#[test]
fn audio_strip_metadata_invalid() {
    assert!(AudioStripMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn audio_strip_metadata_no_input() {
    assert!(AudioStripMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn audio_strip_metadata_id() {
    assert_eq!(AudioStripMetadataFn.id().name, "audio_strip_metadata");
}
