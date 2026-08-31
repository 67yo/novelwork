//! 章节正文朗读：kokoro-tts 合成，voxudio 本机播放。

use crate::paths::embed_models_dir;
use futures::StreamExt;
use kokoro_tts::{KokoroTts, Voice};
use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use voxudio::AudioPlayer;

const MODEL_URL: &str =
    "https://github.com/mzdk100/kokoro/releases/download/V1.1/kokoro-v1.1-zh.onnx";
const VOICES_URL: &str = "https://github.com/mzdk100/kokoro/releases/download/V1.1/voices-v1.1-zh.bin";
const MODEL_NAME: &str = "kokoro-v1.1-zh.onnx";
const VOICES_NAME: &str = "voices-v1.1-zh.bin";
/// onnx ≈ 88 MB / voices ≈ 27 MB
const MODEL_WEIGHT: f64 = 0.765;
const VOICES_WEIGHT: f64 = 0.235;
const SAMPLE_RATE: usize = 24_000;
const CHUNK_CHARS: usize = 160;
const CANCELLED: &str = "cancelled";

fn clamp_speed(speed: i32) -> i32 {
    speed.clamp(1, 3)
}

static PLAY_GEN: AtomicU64 = AtomicU64::new(0);
static ENGINE: Lazy<Mutex<Option<Arc<KokoroTts>>>> = Lazy::new(|| Mutex::new(None));

fn kokoro_dir() -> PathBuf {
    let p = embed_models_dir().join("kokoro");
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn stop() {
    PLAY_GEN.fetch_add(1, Ordering::SeqCst);
}

fn current_gen() -> u64 {
    PLAY_GEN.load(Ordering::SeqCst)
}

/// 去掉 Markdown 记号，便于朗读。
pub fn for_speech(md: &str) -> String {
    let mut out = String::new();
    for line in md.replace('\r', "").lines() {
        let t = line
            .trim()
            .trim_start_matches('#')
            .replace("**", "")
            .replace("__", "")
            .replace('`', "")
            .replace('*', "")
            .replace('>', "")
            .trim()
            .to_string();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with(['。', '！', '？', '.', '!', '?', '\n']) {
            out.push('。');
        }
        out.push_str(&t);
    }
    out
}

struct SpeechSpan {
    speech: String,
    /// UTF-16 offsets into the source passed to `play_text` (textarea).
    start: usize,
    end: usize,
}

fn flush_span(buf: &mut String, start: usize, end: usize, spans: &mut Vec<SpeechSpan>) {
    let raw = std::mem::take(buf);
    let speech = for_speech(&raw);
    if speech.trim().is_empty() {
        return;
    }
    spans.push(SpeechSpan { speech, start, end });
}

/// 按句切块，并记下原文 UTF-16 范围，便于播放时选中。
fn speech_spans(text: &str, max_chars: usize) -> Vec<SpeechSpan> {
    let max_chars = max_chars.max(20);
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut buf_start = 0usize;
    let mut u16_at = 0usize;
    for ch in text.chars() {
        if buf.is_empty() {
            buf_start = u16_at;
        }
        buf.push(ch);
        u16_at += ch.len_utf16();
        let n = buf.chars().count();
        let punct = "。！？!?；;\n".contains(ch);
        if (punct && n >= 12) || n >= max_chars {
            flush_span(&mut buf, buf_start, u16_at, &mut spans);
        }
    }
    if !buf.is_empty() {
        flush_span(&mut buf, buf_start, u16_at, &mut spans);
    }
    spans
}

/// 按句切块，避免一次推理过长。
pub fn speech_chunks(s: &str, max_chars: usize) -> Vec<String> {
    speech_spans(s, max_chars)
        .into_iter()
        .map(|s| s.speech)
        .collect()
}

fn file_ready(path: &Path) -> bool {
    path.is_file() && path.metadata().map(|m| m.len()).unwrap_or(0) > 1024
}

fn download_percent(weight_before: f64, weight: f64, frac: f64) -> u32 {
    ((weight_before + weight * frac.clamp(0.0, 1.0)) * 90.0).round() as u32
}

fn emit_download(app: &tauri::AppHandle, payload: serde_json::Value) {
    use tauri::Emitter;
    let _ = app.emit("chapter-tts-download", payload);
}

fn emit_file_progress(
    app: &tauri::AppHandle,
    file: &str,
    file_index: usize,
    done: u64,
    total: Option<u64>,
    percent: u32,
) {
    emit_download(
        app,
        serde_json::json!({
            "phase": "download",
            "file": file,
            "fileIndex": file_index,
            "fileTotal": 2,
            "bytesDownloaded": done,
            "bytesTotal": total,
            "percent": percent.min(90),
        }),
    );
}

fn download_file_blocking(
    app: &tauri::AppHandle,
    url: &str,
    dest: &Path,
    file: &str,
    file_index: usize,
    weight_before: f64,
    weight: f64,
    gen: u64,
) -> Result<(), String> {
    use std::io::{Read, Write};
    if file_ready(dest) {
        return Ok(());
    }
    if current_gen() != gen {
        return Err(CANCELLED.into());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = dest.with_extension("download");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("novework")
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("下载朗读模型失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("下载朗读模型失败: {e}"))?;
    let total = resp.content_length();
    let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last_emit = 0u64;
    emit_file_progress(app, file, file_index, 0, total, download_percent(weight_before, weight, 0.0));
    let result = (|| -> Result<(), String> {
        loop {
            if current_gen() != gen {
                return Err(CANCELLED.into());
            }
            let n = resp.read(&mut buf).map_err(|e| format!("下载朗读模型失败: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            let frac = match total {
                Some(t) if t > 0 => done as f64 / t as f64,
                _ => 0.0,
            };
            let step = total.map(|t| (t / 50).max(256 * 1024)).unwrap_or(512 * 1024);
            if done - last_emit >= step || total == Some(done) {
                last_emit = done;
                emit_file_progress(
                    app,
                    file,
                    file_index,
                    done,
                    total,
                    download_percent(weight_before, weight, frac),
                );
            }
        }
        Ok(())
    })();
    if result.is_err() {
        drop(out);
        let _ = std::fs::remove_file(&tmp);
        return result;
    }
    emit_file_progress(
        app,
        file,
        file_index,
        done,
        total.or(Some(done)),
        download_percent(weight_before, weight, 1.0),
    );
    out.sync_all().ok();
    drop(out);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_file(
    app: &tauri::AppHandle,
    url: &'static str,
    dest: PathBuf,
    file: &'static str,
    file_index: usize,
    weight_before: f64,
    weight: f64,
    gen: u64,
) -> Result<(), String> {
    if file_ready(&dest) {
        return Ok(());
    }
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        download_file_blocking(&app, url, &dest, file, file_index, weight_before, weight, gen)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn ensure_models(
    app: &tauri::AppHandle,
    gen: u64,
) -> Result<(PathBuf, PathBuf, bool), String> {
    let dir = kokoro_dir();
    let model = dir.join(MODEL_NAME);
    let voices = dir.join(VOICES_NAME);
    let need = !file_ready(&model) || !file_ready(&voices);
    if need {
        download_file(app, MODEL_URL, model.clone(), MODEL_NAME, 1, 0.0, MODEL_WEIGHT, gen)
            .await?;
        if current_gen() != gen {
            return Err(CANCELLED.into());
        }
        download_file(
            app,
            VOICES_URL,
            voices.clone(),
            VOICES_NAME,
            2,
            MODEL_WEIGHT,
            VOICES_WEIGHT,
            gen,
        )
        .await?;
    }
    Ok((model, voices, need))
}

async fn engine(app: &tauri::AppHandle, gen: u64) -> Result<Arc<KokoroTts>, String> {
    if current_gen() != gen {
        return Err(CANCELLED.into());
    }
    let mut slot = ENGINE.lock().await;
    if let Some(e) = slot.as_ref() {
        return Ok(e.clone());
    }
    let (model, voices, downloaded) = ensure_models(app, gen).await?;
    if current_gen() != gen {
        return Err(CANCELLED.into());
    }
    if downloaded {
        emit_download(app, serde_json::json!({ "phase": "load", "percent": 92 }));
    }
    let tts = KokoroTts::new(&model, &voices)
        .await
        .map_err(|e| format!("加载朗读模型失败: {e}"))?;
    if downloaded {
        emit_download(app, serde_json::json!({ "phase": "done", "percent": 100 }));
    }
    let arc = Arc::new(tts);
    *slot = Some(arc.clone());
    Ok(arc)
}

fn emit_status(app: &tauri::AppHandle, status: &str) {
    use tauri::Emitter;
    let _ = app.emit("chapter-tts-status", serde_json::json!({ "status": status }));
}

fn emit_chunk(app: &tauri::AppHandle, start: usize, end: usize) {
    use tauri::Emitter;
    let _ = app.emit(
        "chapter-tts-chunk",
        serde_json::json!({ "start": start, "end": end }),
    );
}

fn finish_download_ui(app: &tauri::AppHandle) {
    emit_download(app, serde_json::json!({ "phase": "done", "percent": 100 }));
}

pub async fn play_text(app: &tauri::AppHandle, text: &str, speed: i32) -> Result<(), String> {
    stop();
    let gen = PLAY_GEN.load(Ordering::SeqCst);
    let speed = clamp_speed(speed);
    let spans = speech_spans(text, CHUNK_CHARS);
    if spans.is_empty() {
        return Err("正文为空".into());
    }
    let chunks: Vec<String> = spans.iter().map(|s| s.speech.clone()).collect();
    let ranges: Vec<(usize, usize)> = spans.iter().map(|s| (s.start, s.end)).collect();
    if current_gen() != gen {
        return Ok(());
    }
    let tts = match engine(app, gen).await {
        Ok(t) => t,
        Err(e) if e == CANCELLED => {
            finish_download_ui(app);
            return Ok(());
        }
        Err(e) => {
            finish_download_ui(app);
            return Err(e);
        }
    };
    if current_gen() != gen {
        finish_download_ui(app);
        return Ok(());
    }
    let mut player = AudioPlayer::new().map_err(|e| format!("无法打开扬声器: {e}"))?;
    player
        .play()
        .map_err(|e| format!("无法开始播放: {e}"))?;
    emit_status(app, "playing");
    let (mut sink, mut stream) = tts.stream(Voice::Zm045(speed));
    let feeder = tokio::spawn(async move {
        for c in chunks {
            if current_gen() != gen {
                break;
            }
            if sink.synth(c).await.is_err() {
                break;
            }
        }
        drop(sink);
    });
    let mut idx = 0usize;
    while let Some((audio, _)) = stream.next().await {
        if current_gen() != gen {
            break;
        }
        if let Some(&(start, end)) = ranges.get(idx) {
            emit_chunk(app, start, end);
        }
        idx += 1;
        if player
            .write::<SAMPLE_RATE, _>(&audio, 1)
            .await
            .is_err()
        {
            break;
        }
    }
    let _ = feeder.await;
    if current_gen() == gen {
        emit_status(app, "idle");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_strips_md_and_chunks() {
        let s = for_speech("# 第一章\n\n**风起了。**他走了！\n\n后一句。");
        assert!(s.contains("风起了"));
        assert!(!s.contains('*'));
        let parts = speech_chunks(&s, 20);
        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|p| p.chars().count() <= 20 + 8));
    }

    fn utf16_slice(s: &str, start: usize, end: usize) -> String {
        let u: Vec<u16> = s
            .encode_utf16()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        String::from_utf16_lossy(&u)
    }

    #[test]
    fn speech_spans_utf16_match_source() {
        let md = "他轻轻说了一句话。随后天色就亮了！还有第三句。";
        let spans = speech_spans(md, 20);
        assert!(spans.len() >= 2);
        for sp in &spans {
            assert!(sp.end > sp.start);
            let slice = utf16_slice(md, sp.start, sp.end);
            assert_eq!(for_speech(&slice), sp.speech);
        }
    }

    #[test]
    fn download_percent_maps_file_weights_into_0_90() {
        assert_eq!(download_percent(0.0, MODEL_WEIGHT, 0.0), 0);
        assert_eq!(download_percent(0.0, MODEL_WEIGHT, 1.0), 69);
        assert_eq!(download_percent(MODEL_WEIGHT, VOICES_WEIGHT, 1.0), 90);
    }

    #[test]
    fn clamp_speed_keeps_v11_range() {
        assert_eq!(clamp_speed(0), 1);
        assert_eq!(clamp_speed(1), 1);
        assert_eq!(clamp_speed(2), 2);
        assert_eq!(clamp_speed(3), 3);
        assert_eq!(clamp_speed(9), 3);
    }
}
