use bytes::Bytes;
use std::collections::BTreeMap;
use std::io::Cursor;
use deriva_core::address::{FunctionId, Value};
use crate::function::{ComputeCost, ComputeError, ComputeFunction};

fn fid(name: &str) -> FunctionId { FunctionId { name: name.into(), version: "1.0.0".into() } }
fn param_str(p: &BTreeMap<String, Value>, k: &str) -> Option<String> {
    match p.get(k) { Some(Value::String(s)) => Some(s.clone()), _ => None }
}
fn one(inputs: &[Bytes]) -> Result<&Bytes, ComputeError> {
    inputs.first().ok_or_else(|| ComputeError::InvalidParam("input required".into()))
}
fn fail(msg: String) -> ComputeError { ComputeError::ExecutionFailed(msg) }
fn cost(sizes: &[u64]) -> ComputeCost {
    let total: u64 = sizes.iter().sum();
    ComputeCost { cpu_ms: (total / 1024).max(10), memory_bytes: total.max(1024) }
}

// --- symphonia helpers ---

use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};

struct CursorSource(Cursor<Vec<u8>>);
impl std::io::Read for CursorSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { self.0.read(buf) }
}
impl std::io::Seek for CursorSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> { self.0.seek(pos) }
}
impl MediaSource for CursorSource {
    fn is_seekable(&self) -> bool { true }
    fn byte_len(&self) -> Option<u64> { Some(self.0.get_ref().len() as u64) }
}

fn make_mss(data: &[u8]) -> MediaSourceStream {
    MediaSourceStream::new(
        Box::new(CursorSource(Cursor::new(data.to_vec()))),
        MediaSourceStreamOptions::default(),
    )
}

fn probe_audio(data: &[u8]) -> Result<symphonia::core::probe::ProbeResult, ComputeError> {
    symphonia::default::get_probe()
        .format(&Hint::new(), make_mss(data), &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| fail(format!("probe: {e}")))
}

/// Decode all audio to interleaved i16 samples. Returns (samples, sample_rate, channels).
fn decode_all_i16(data: &[u8]) -> Result<(Vec<i16>, u32, u16), ComputeError> {
    let probed = probe_audio(data)?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| fail("no audio track".into()))?;
    let track_id = track.id;
    let sr = track.codec_params.sample_rate.unwrap_or(44100);
    let ch = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(1);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| fail(format!("decoder: {e}")))?;
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(fail(format!("packet: {e}"))),
        };
        if packet.track_id() != track_id { continue; }
        let decoded = decoder.decode(&packet).map_err(|e| fail(format!("decode: {e}")))?;
        use symphonia::core::audio::AudioBufferRef;
        match decoded {
            AudioBufferRef::S16(buf) => {
                use symphonia::core::audio::Signal;
                for f in 0..buf.frames() {
                    for c in 0..ch as usize { samples.push(buf.chan(c)[f]); }
                }
            }
            AudioBufferRef::S32(buf) => {
                use symphonia::core::audio::Signal;
                for f in 0..buf.frames() {
                    for c in 0..ch as usize { samples.push((buf.chan(c)[f] >> 16) as i16); }
                }
            }
            AudioBufferRef::F32(buf) => {
                use symphonia::core::audio::Signal;
                for f in 0..buf.frames() {
                    for c in 0..ch as usize {
                        samples.push((buf.chan(c)[f] * 32767.0).clamp(-32768.0, 32767.0) as i16);
                    }
                }
            }
            AudioBufferRef::F64(buf) => {
                use symphonia::core::audio::Signal;
                for f in 0..buf.frames() {
                    for c in 0..ch as usize {
                        samples.push((buf.chan(c)[f] * 32767.0).clamp(-32768.0, 32767.0) as i16);
                    }
                }
            }
            _ => {}
        }
    }
    Ok((samples, sr, ch))
}

fn samples_to_wav(samples: &[i16], sr: u32, ch: u16) -> Vec<u8> {
    let bps: u16 = 16;
    let data_len = (samples.len() * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&ch.to_le_bytes());
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&(sr * ch as u32 * bps as u32 / 8).to_le_bytes());
    buf.extend_from_slice(&(ch * bps / 8).to_le_bytes());
    buf.extend_from_slice(&bps.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples { buf.extend_from_slice(&s.to_le_bytes()); }
    buf
}

// ---- #301 AudioMetadataFn ----
pub struct AudioMetadataFn;
impl ComputeFunction for AudioMetadataFn {
    fn id(&self) -> FunctionId { fid("audio_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let probed = probe_audio(b)?;
        let mut format = probed.format;
        let track = format.default_track().ok_or_else(|| fail("no audio track".into()))?;
        let codec = format!("{}", track.codec_params.codec);
        let sr = track.codec_params.sample_rate.unwrap_or(0);
        let ch = track.codec_params.channels.map(|c| c.count()).unwrap_or(0);
        let duration_ms = track.codec_params.n_frames
            .and_then(|n| track.codec_params.time_base.map(|tb| {
                let t = tb.calc_time(n);
                t.seconds * 1000 + (t.frac * 1000.0) as u64
            }));
        let mut tags = serde_json::Map::new();
        if let Some(rev) = format.metadata().current() {
            for tag in rev.tags() {
                tags.insert(tag.key.clone(), serde_json::Value::String(tag.value.to_string()));
            }
        }
        let meta = serde_json::json!({
            "codec": codec, "sample_rate": sr, "channels": ch,
            "duration_ms": duration_ms, "tags": tags,
        });
        Ok(Bytes::from(serde_json::to_string_pretty(&meta).unwrap()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #302 AudioConvertFn ----
pub struct AudioConvertFn;
impl ComputeFunction for AudioConvertFn {
    fn id(&self) -> FunctionId { fid("audio_convert") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let fmt = param_str(p, "format").unwrap_or_else(|| "wav".into());
        let (samples, sr, ch) = decode_all_i16(b)?;
        match fmt.as_str() {
            "wav" => Ok(Bytes::from(samples_to_wav(&samples, sr, ch))),
            _ => Err(ComputeError::InvalidParam(format!("unsupported output format: {fmt} (only wav supported without ffmpeg)"))),
        }
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #303 AudioTrimFn ----
pub struct AudioTrimFn;
impl ComputeFunction for AudioTrimFn {
    fn id(&self) -> FunctionId { fid("audio_trim") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let start_ms: u64 = param_str(p, "start_ms").unwrap_or_else(|| "0".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid start_ms".into()))?;
        let end_ms: u64 = param_str(p, "end_ms")
            .ok_or_else(|| ComputeError::InvalidParam("end_ms required".into()))?
            .parse().map_err(|_| ComputeError::InvalidParam("invalid end_ms".into()))?;
        if start_ms >= end_ms {
            return Err(ComputeError::InvalidParam("start_ms must be < end_ms".into()));
        }
        let (samples, sr, ch) = decode_all_i16(b)?;
        let samples_per_ms = (sr as u64 * ch as u64) / 1000;
        let start_idx = (start_ms * samples_per_ms) as usize;
        let total = samples.len();
        let end_idx = ((end_ms * samples_per_ms) as usize).min(total);
        let start_idx = start_idx.min(total);
        let trimmed = &samples[start_idx..end_idx];
        Ok(Bytes::from(samples_to_wav(trimmed, sr, ch)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #304 AudioNormalizeFn ----
pub struct AudioNormalizeFn;
impl ComputeFunction for AudioNormalizeFn {
    fn id(&self) -> FunctionId { fid("audio_normalize") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let target_db: f64 = param_str(p, "target_db").unwrap_or_else(|| "-3".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid target_db".into()))?;
        let (mut samples, sr, ch) = decode_all_i16(b)?;
        let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        if peak == 0 { return Ok(Bytes::from(samples_to_wav(&samples, sr, ch))); }
        let target_linear = 32767.0 * 10f64.powf(target_db / 20.0);
        let gain = target_linear / peak as f64;
        for s in &mut samples {
            *s = ((*s as f64) * gain).clamp(-32768.0, 32767.0) as i16;
        }
        Ok(Bytes::from(samples_to_wav(&samples, sr, ch)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #305 AudioWaveformFn ----
pub struct AudioWaveformFn;
impl ComputeFunction for AudioWaveformFn {
    fn id(&self) -> FunctionId { fid("audio_waveform") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let width: u32 = param_str(p, "width").unwrap_or_else(|| "800".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid width".into()))?;
        let height: u32 = param_str(p, "height").unwrap_or_else(|| "200".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid height".into()))?;
        let (samples, _sr, ch) = decode_all_i16(b)?;
        // Mix to mono
        let mono: Vec<i16> = if ch <= 1 {
            samples
        } else {
            samples.chunks(ch as usize)
                .map(|c| (c.iter().map(|&s| s as i32).sum::<i32>() / ch as i32) as i16)
                .collect()
        };
        let chunk_size = (mono.len() / width as usize).max(1);
        let mut img = image::RgbaImage::new(width, height);
        for pixel in img.pixels_mut() { *pixel = image::Rgba([0, 0, 0, 255]); }
        let mid = height / 2;
        for x in 0..width as usize {
            let start = x * chunk_size;
            let end = (start + chunk_size).min(mono.len());
            if start >= mono.len() { break; }
            let chunk = &mono[start..end];
            let mn = *chunk.iter().min().unwrap_or(&0) as f32 / 32768.0;
            let mx = *chunk.iter().max().unwrap_or(&0) as f32 / 32768.0;
            let y_top = (mid as f32 - mx * mid as f32) as u32;
            let y_bot = (mid as f32 - mn * mid as f32) as u32;
            for y in y_top.min(height - 1)..=y_bot.min(height - 1) {
                img.put_pixel(x as u32, y, image::Rgba([0, 200, 100, 255]));
            }
        }
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| fail(format!("png: {e}")))?;
        Ok(Bytes::from(out.into_inner()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #306 AudioSilenceDetectFn ----
pub struct AudioSilenceDetectFn;
impl ComputeFunction for AudioSilenceDetectFn {
    fn id(&self) -> FunctionId { fid("audio_silence_detect") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let threshold_db: f64 = param_str(p, "threshold_db").unwrap_or_else(|| "-40".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid threshold_db".into()))?;
        let min_dur_ms: u64 = param_str(p, "min_duration_ms").unwrap_or_else(|| "500".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid min_duration_ms".into()))?;
        let (samples, sr, ch) = decode_all_i16(b)?;
        let threshold = (32768.0 * 10f64.powf(threshold_db / 20.0)) as u16;
        let mono: Vec<i16> = if ch <= 1 { samples } else {
            samples.chunks(ch as usize)
                .map(|c| (c.iter().map(|&s| s as i32).sum::<i32>() / ch as i32) as i16)
                .collect()
        };
        let samples_per_ms = sr as f64 / 1000.0;
        let mut regions = Vec::new();
        let mut silence_start: Option<usize> = None;
        for (i, &s) in mono.iter().enumerate() {
            let is_silent = s.unsigned_abs() <= threshold;
            match (is_silent, silence_start) {
                (true, None) => silence_start = Some(i),
                (false, Some(start)) => {
                    let dur_ms = ((i - start) as f64 / samples_per_ms) as u64;
                    if dur_ms >= min_dur_ms {
                        let start_ms = (start as f64 / samples_per_ms) as u64;
                        let end_ms = (i as f64 / samples_per_ms) as u64;
                        regions.push(serde_json::json!({"start_ms": start_ms, "end_ms": end_ms}));
                    }
                    silence_start = None;
                }
                _ => {}
            }
        }
        if let Some(start) = silence_start {
            let dur_ms = ((mono.len() - start) as f64 / samples_per_ms) as u64;
            if dur_ms >= min_dur_ms {
                let start_ms = (start as f64 / samples_per_ms) as u64;
                let end_ms = (mono.len() as f64 / samples_per_ms) as u64;
                regions.push(serde_json::json!({"start_ms": start_ms, "end_ms": end_ms}));
            }
        }
        Ok(Bytes::from(serde_json::to_string(&regions).unwrap()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #307 AudioStripMetadataFn ----
pub struct AudioStripMetadataFn;
impl ComputeFunction for AudioStripMetadataFn {
    fn id(&self) -> FunctionId { fid("audio_strip_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Decode to PCM and re-encode as WAV (strips all tags)
        let (samples, sr, ch) = decode_all_i16(b)?;
        Ok(Bytes::from(samples_to_wav(&samples, sr, ch)))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #308 VideoMetadataFn ----
pub struct VideoMetadataFn;
impl ComputeFunction for VideoMetadataFn {
    fn id(&self) -> FunctionId { fid("video_metadata") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        // Use symphonia to probe container — it can parse MP4/MKV containers
        let probed = probe_audio(b)?;
        let format = probed.format;
        let tracks: Vec<serde_json::Value> = format.tracks().iter().map(|t| {
            serde_json::json!({
                "id": t.id,
                "codec": format!("{}", t.codec_params.codec),
                "sample_rate": t.codec_params.sample_rate,
                "channels": t.codec_params.channels.map(|c| c.count()),
            })
        }).collect();
        let meta = serde_json::json!({ "tracks": tracks, "track_count": tracks.len() });
        Ok(Bytes::from(serde_json::to_string_pretty(&meta).unwrap()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #309 VideoThumbnailFn (requires ffmpeg) ----
pub struct VideoThumbnailFn;
impl ComputeFunction for VideoThumbnailFn {
    fn id(&self) -> FunctionId { fid("video_thumbnail") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("video_thumbnail requires format-media-ffmpeg feature".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #310 VideoExtractAudioFn (requires ffmpeg) ----
pub struct VideoExtractAudioFn;
impl ComputeFunction for VideoExtractAudioFn {
    fn id(&self) -> FunctionId { fid("video_extract_audio") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("video_extract_audio requires format-media-ffmpeg feature".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #311 VideoStripAudioFn (requires ffmpeg) ----
pub struct VideoStripAudioFn;
impl ComputeFunction for VideoStripAudioFn {
    fn id(&self) -> FunctionId { fid("video_strip_audio") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("video_strip_audio requires format-media-ffmpeg feature".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #312 VideoResolutionFn (requires ffmpeg) ----
pub struct VideoResolutionFn;
impl ComputeFunction for VideoResolutionFn {
    fn id(&self) -> FunctionId { fid("video_resolution") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("video_resolution requires format-media-ffmpeg feature".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #313 SubtitleExtractFn (requires ffmpeg) ----
pub struct SubtitleExtractFn;
impl ComputeFunction for SubtitleExtractFn {
    fn id(&self) -> FunctionId { fid("subtitle_extract") }
    fn execute(&self, _inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        Err(fail("subtitle_extract requires format-media-ffmpeg feature".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #314 WavToRawPcmFn ----
pub struct WavToRawPcmFn;
impl ComputeFunction for WavToRawPcmFn {
    fn id(&self) -> FunctionId { fid("wav_to_raw_pcm") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return Err(fail("not a WAV file".into()));
        }
        // Parse fmt chunk
        let audio_fmt = u16::from_le_bytes([b[20], b[21]]);
        if audio_fmt != 1 {
            return Err(fail("only PCM WAV supported".into()));
        }
        let channels = u16::from_le_bytes([b[22], b[23]]);
        let sample_rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
        let bits_per_sample = u16::from_le_bytes([b[34], b[35]]);
        // Find data chunk
        let mut pos = 12;
        while pos + 8 <= b.len() {
            let chunk_id = &b[pos..pos + 4];
            let chunk_size = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
            if chunk_id == b"data" {
                let data_start = pos + 8;
                let data_end = (data_start + chunk_size).min(b.len());
                let header = serde_json::json!({
                    "sample_rate": sample_rate, "channels": channels,
                    "bits_per_sample": bits_per_sample,
                });
                let header_bytes = serde_json::to_string(&header).unwrap();
                let mut out = Vec::with_capacity(header_bytes.len() + 1 + (data_end - data_start));
                out.extend_from_slice(header_bytes.as_bytes());
                out.push(b'\n');
                out.extend_from_slice(&b[data_start..data_end]);
                return Ok(Bytes::from(out));
            }
            pos += 8 + chunk_size;
            if chunk_size % 2 != 0 { pos += 1; } // padding
        }
        Err(fail("no data chunk in WAV".into()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #315 RawPcmToWavFn ----
pub struct RawPcmToWavFn;
impl ComputeFunction for RawPcmToWavFn {
    fn id(&self) -> FunctionId { fid("raw_pcm_to_wav") }
    fn execute(&self, inputs: Vec<Bytes>, p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let sr: u32 = param_str(p, "sample_rate").unwrap_or_else(|| "44100".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid sample_rate".into()))?;
        let ch: u16 = param_str(p, "channels").unwrap_or_else(|| "1".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid channels".into()))?;
        let bps: u16 = param_str(p, "bits_per_sample").unwrap_or_else(|| "16".into())
            .parse().map_err(|_| ComputeError::InvalidParam("invalid bits_per_sample".into()))?;
        let data_len = b.len() as u32;
        let mut wav = Vec::with_capacity(44 + b.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&ch.to_le_bytes());
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * ch as u32 * bps as u32 / 8).to_le_bytes());
        wav.extend_from_slice(&(ch * bps / 8).to_le_bytes());
        wav.extend_from_slice(&bps.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(b);
        Ok(Bytes::from(wav))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}

// ---- #316 MediaDetectFormatFn ----
pub struct MediaDetectFormatFn;
impl ComputeFunction for MediaDetectFormatFn {
    fn id(&self) -> FunctionId { fid("media_detect_format") }
    fn execute(&self, inputs: Vec<Bytes>, _p: &BTreeMap<String, Value>) -> Result<Bytes, ComputeError> {
        let b = one(&inputs)?;
        let (format, mime) = if b.len() >= 4 && &b[0..4] == b"RIFF" && b.len() >= 12 && &b[8..12] == b"WAVE" {
            ("wav", "audio/wav")
        } else if b.len() >= 4 && &b[0..4] == b"fLaC" {
            ("flac", "audio/flac")
        } else if b.len() >= 3 && (b[0] == 0xFF && (b[1] & 0xE0) == 0xE0) {
            ("mp3", "audio/mpeg")
        } else if b.len() >= 2 && &b[0..2] == b"ID" && b.len() >= 3 && b[2] == 3 {
            ("mp3", "audio/mpeg")
        } else if b.len() >= 4 && &b[0..4] == b"OggS" {
            ("ogg", "audio/ogg")
        } else if b.len() >= 8 && &b[4..8] == b"ftyp" {
            ("mp4", "video/mp4")
        } else if b.len() >= 4 && &b[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
            ("mkv", "video/x-matroska")
        } else {
            ("unknown", "application/octet-stream")
        };
        let meta = serde_json::json!({ "format": format, "mime": mime });
        Ok(Bytes::from(serde_json::to_string(&meta).unwrap()))
    }
    fn estimated_cost(&self, input_sizes: &[u64]) -> ComputeCost {
        cost(input_sizes)
    }
}
