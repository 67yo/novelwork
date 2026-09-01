//! 本机 AI / Chat 交互日志（设置开关）。按日 JSONL，不含 API Key。

use chrono::Local;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static RUN_ID: Mutex<Option<String>> = Mutex::new(None);
static WRITE: Mutex<()> = Mutex::new(());

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_run(id: Option<String>) {
    *RUN_ID.lock() = id;
}

/// 一次 Chat/AI 会话的关联 id；drop 时清掉。
pub struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) {
        set_run(None);
    }
}

pub fn begin_run() -> RunGuard {
    set_run(Some(uuid::Uuid::new_v4().to_string()));
    RunGuard
}

pub fn dir() -> PathBuf {
    crate::paths::app_root().join("logs").join("ai")
}

fn ensure_dir() -> PathBuf {
    let p = dir();
    fs::create_dir_all(&p).ok();
    p
}

pub fn redact_value(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let kl = k.to_ascii_lowercase();
                if kl.contains("api_key")
                    || kl.contains("apikey")
                    || kl == "authorization"
                    || kl == "token"
                {
                    map.insert(k, json!("***"));
                    continue;
                }
                if let Some(child) = map.get_mut(&k) {
                    redact_value(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item);
            }
        }
        Value::String(s) => {
            if s.starts_with("sk-") || s.starts_with("Bearer ") {
                *s = "***".into();
            }
        }
        _ => {}
    }
}

pub fn error_chain(err: &dyn std::error::Error) -> Vec<String> {
    let mut out = vec![err.to_string()];
    let mut src = err.source();
    while let Some(e) = src {
        out.push(e.to_string());
        src = e.source();
    }
    out
}

/// 二进制数组压成长度，避免把图片写进日志。
fn shrink_binaries(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for child in map.values_mut() {
                shrink_binaries(child);
            }
        }
        Value::Array(items) => {
            if items.len() > 64 && items.iter().all(|x| x.is_number()) {
                *v = json!({ "_bytes": items.len() });
                return;
            }
            for item in items {
                shrink_binaries(item);
            }
        }
        _ => {}
    }
}

pub fn write(kind: &str, mut payload: Value) {
    if !enabled() {
        return;
    }
    redact_value(&mut payload);
    shrink_binaries(&mut payload);
    let mut rec = serde_json::Map::new();
    rec.insert("ts".into(), json!(Local::now().to_rfc3339()));
    rec.insert("kind".into(), json!(kind));
    if let Some(id) = RUN_ID.lock().clone() {
        rec.insert("run".into(), json!(id));
    }
    if let Some(obj) = payload.as_object_mut() {
        rec.append(obj);
    } else {
        rec.insert("data".into(), payload);
    }
    let line = match serde_json::to_string(&Value::Object(rec)) {
        Ok(s) => s,
        Err(_) => return,
    };
    let path = ensure_dir().join(format!("ai-{}.jsonl", Local::now().format("%Y-%m-%d")));
    let _g = WRITE.lock();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 打开日志目录（不存在则创建）。
pub fn open_dir() -> Result<PathBuf, String> {
    let p = ensure_dir();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_strips_keys_and_bearer() {
        let mut v = json!({
            "api_key": "secret",
            "headers": { "Authorization": "Bearer abc" },
            "ok": "yes",
            "token": "t"
        });
        redact_value(&mut v);
        assert_eq!(v["api_key"], json!("***"));
        assert_eq!(v["headers"]["Authorization"], json!("***"));
        assert_eq!(v["token"], json!("***"));
        assert_eq!(v["ok"], json!("yes"));
    }

    #[test]
    fn shrink_collapses_byte_arrays() {
        let mut v = json!({"data": (0..80).collect::<Vec<u8>>()});
        shrink_binaries(&mut v);
        assert_eq!(v["data"]["_bytes"], 80);
    }

    #[test]
    fn write_noop_when_disabled() {
        set_enabled(false);
        write("test", json!({"x": 1}));
    }
}
