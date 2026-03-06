use bytes::Bytes;
use deriva_compute::builtins_format_audio::*;
use deriva_compute::function::ComputeFunction;
use super::helpers::*;

// ---- video_metadata (5 tests) ----
#[test]
fn video_metadata_wav_tracks() {
    // symphonia can probe WAV and report its track
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = VideoMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["track_count"].as_u64().unwrap() >= 1);
}

#[test]
fn video_metadata_invalid() {
    assert!(VideoMetadataFn.execute(vec![Bytes::from_static(b"bad")], &p(&[])).is_err());
}

#[test]
fn video_metadata_no_input() {
    assert!(VideoMetadataFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn video_metadata_has_tracks_array() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = VideoMetadataFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(v["tracks"].is_array());
}

#[test]
fn video_metadata_id() {
    assert_eq!(VideoMetadataFn.id().name, "video_metadata");
}

// ---- video_thumbnail (5 tests — all stubbed, requires ffmpeg) ----
#[test]
fn video_thumbnail_requires_ffmpeg() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let err = VideoThumbnailFn.execute(vec![wav], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("ffmpeg"));
}

#[test]
fn video_thumbnail_no_input_still_errors_ffmpeg() {
    assert!(VideoThumbnailFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn video_thumbnail_id() {
    assert_eq!(VideoThumbnailFn.id().name, "video_thumbnail");
}

#[test]
fn video_thumbnail_with_time_param() {
    let err = VideoThumbnailFn.execute(vec![Bytes::new()], &p(&[("time_ms", "1000")])).unwrap_err();
    assert!(format!("{err:?}").contains("ffmpeg"));
}

#[test]
fn video_thumbnail_error_type() {
    let err = VideoThumbnailFn.execute(vec![Bytes::new()], &p(&[]));
    assert!(err.is_err());
}

// ---- video_extract_audio (5 tests — stubbed) ----
#[test]
fn video_extract_audio_requires_ffmpeg() {
    assert!(format!("{:?}", VideoExtractAudioFn.execute(vec![Bytes::new()], &p(&[]))).contains("ffmpeg"));
}

#[test]
fn video_extract_audio_id() {
    assert_eq!(VideoExtractAudioFn.id().name, "video_extract_audio");
}

#[test]
fn video_extract_audio_with_format() {
    assert!(VideoExtractAudioFn.execute(vec![Bytes::new()], &p(&[("format", "wav")])).is_err());
}

#[test]
fn video_extract_audio_no_input() {
    assert!(VideoExtractAudioFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn video_extract_audio_error_msg() {
    let err = VideoExtractAudioFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("format-media-ffmpeg"));
}

// ---- video_strip_audio (5 tests — stubbed) ----
#[test]
fn video_strip_audio_requires_ffmpeg() {
    assert!(VideoStripAudioFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

#[test]
fn video_strip_audio_id() {
    assert_eq!(VideoStripAudioFn.id().name, "video_strip_audio");
}

#[test]
fn video_strip_audio_error_msg() {
    let err = VideoStripAudioFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("ffmpeg"));
}

#[test]
fn video_strip_audio_no_input() {
    assert!(VideoStripAudioFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn video_strip_audio_with_data() {
    assert!(VideoStripAudioFn.execute(vec![Bytes::from_static(b"data")], &p(&[])).is_err());
}

// ---- video_resolution (5 tests — stubbed) ----
#[test]
fn video_resolution_requires_ffmpeg() {
    assert!(VideoResolutionFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

#[test]
fn video_resolution_id() {
    assert_eq!(VideoResolutionFn.id().name, "video_resolution");
}

#[test]
fn video_resolution_error_msg() {
    let err = VideoResolutionFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("ffmpeg"));
}

#[test]
fn video_resolution_no_input() {
    assert!(VideoResolutionFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn video_resolution_with_params() {
    assert!(VideoResolutionFn.execute(vec![Bytes::new()], &p(&[("width", "1920"), ("height", "1080")])).is_err());
}

// ---- subtitle_extract (5 tests — stubbed) ----
#[test]
fn subtitle_extract_requires_ffmpeg() {
    assert!(SubtitleExtractFn.execute(vec![Bytes::new()], &p(&[])).is_err());
}

#[test]
fn subtitle_extract_id() {
    assert_eq!(SubtitleExtractFn.id().name, "subtitle_extract");
}

#[test]
fn subtitle_extract_error_msg() {
    let err = SubtitleExtractFn.execute(vec![Bytes::new()], &p(&[])).unwrap_err();
    assert!(format!("{err:?}").contains("ffmpeg"));
}

#[test]
fn subtitle_extract_no_input() {
    assert!(SubtitleExtractFn.execute(vec![], &p(&[])).is_err());
}

#[test]
fn subtitle_extract_with_track() {
    assert!(SubtitleExtractFn.execute(vec![Bytes::new()], &p(&[("track", "1")])).is_err());
}

// ---- media_detect_format (5 tests) ----
#[test]
fn media_detect_format_wav() {
    let wav = make_wav(440.0, 100, 44100, 1);
    let out = MediaDetectFormatFn.execute(vec![wav], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "wav");
    assert_eq!(v["mime"], "audio/wav");
}

#[test]
fn media_detect_format_flac_magic() {
    let mut data = vec![0u8; 100];
    data[0..4].copy_from_slice(b"fLaC");
    let out = MediaDetectFormatFn.execute(vec![Bytes::from(data)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "flac");
}

#[test]
fn media_detect_format_ogg_magic() {
    let mut data = vec![0u8; 100];
    data[0..4].copy_from_slice(b"OggS");
    let out = MediaDetectFormatFn.execute(vec![Bytes::from(data)], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "ogg");
}

#[test]
fn media_detect_format_unknown() {
    let out = MediaDetectFormatFn.execute(vec![Bytes::from_static(b"random")], &p(&[])).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["format"], "unknown");
}

#[test]
fn media_detect_format_id() {
    assert_eq!(MediaDetectFormatFn.id().name, "media_detect_format");
}
