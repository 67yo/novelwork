//! Chapter → storyboard shots → ComfyUI Desktop (MiniMax) queue.

use crate::paths::novel_dir;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterShot {
    pub id: String,
    pub order: u32,
    pub action: String,
    pub camera: String,
    #[serde(default)]
    pub dialogue: String,
    #[serde(default)]
    pub duration_sec: u8,
    #[serde(default)]
    pub comfy_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChapterShotsFile {
    #[serde(default)]
    pub shots: Vec<ChapterShot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfySubmitResult {
    pub queued: usize,
    pub prompt_ids: Vec<String>,
    pub mode: String,
    pub url: String,
}

pub fn shots_path(novel_id: &str, node_id: &str) -> PathBuf {
    let dir = novel_dir(novel_id).join("shots");
    fs::create_dir_all(&dir).ok();
    dir.join(format!("{node_id}.json"))
}

pub fn load_shots(novel_id: &str, node_id: &str) -> Result<Vec<ChapterShot>, String> {
    let p = shots_path(novel_id, node_id);
    if !p.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let file: ChapterShotsFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(file.shots)
}

pub fn save_shots(novel_id: &str, node_id: &str, shots: Vec<ChapterShot>) -> Result<Vec<ChapterShot>, String> {
    let mut shots = shots;
    for (i, s) in shots.iter_mut().enumerate() {
        s.order = (i as u32) + 1;
        if s.id.trim().is_empty() {
            s.id = Uuid::new_v4().to_string();
        }
        if s.duration_sec == 0 {
            s.duration_sec = 8;
        }
        s.duration_sec = s.duration_sec.clamp(5, 15);
    }
    let p = shots_path(novel_id, node_id);
    let body = serde_json::to_string_pretty(&ChapterShotsFile {
        shots: shots.clone(),
    })
    .map_err(|e| e.to_string())?;
    fs::write(p, body).map_err(|e| e.to_string())?;
    Ok(shots)
}

pub fn parse_shots_reply(text: &str) -> Result<Vec<ChapterShot>, String> {
    let arr = first_shots_array(text).ok_or_else(|| "分镜头 JSON 解析失败".to_string())?;
    let mut out = Vec::new();
    for (i, v) in arr.iter().enumerate() {
        let action = v
            .get("action")
            .or_else(|| v.get("summary"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if action.is_empty() {
            continue;
        }
        let duration = v
            .get("duration_sec")
            .and_then(|x| x.as_u64())
            .unwrap_or(8)
            .clamp(5, 15) as u8;
        out.push(ChapterShot {
            id: Uuid::new_v4().to_string(),
            order: (i as u32) + 1,
            action,
            camera: v
                .get("camera")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            dialogue: v
                .get("dialogue")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
            duration_sec: duration,
            comfy_prompt: v
                .get("comfy_prompt")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
        });
    }
    if out.is_empty() {
        return Err("模型没有返回可用分镜头".into());
    }
    Ok(out)
}

pub fn apply_prompt_reply(existing: &[ChapterShot], text: &str) -> Result<Vec<ChapterShot>, String> {
    let arr = first_shots_array(text).ok_or_else(|| "提示词 JSON 解析失败".to_string())?;
    let mut out = existing.to_vec();
    for v in arr {
        let order = v.get("order").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let prompt = v
            .get("comfy_prompt")
            .or_else(|| v.get("prompt"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if prompt.is_empty() {
            continue;
        }
        if let Some(shot) = out.iter_mut().find(|s| s.order == order) {
            shot.comfy_prompt = prompt;
        }
    }
    if out.iter().all(|s| s.comfy_prompt.trim().is_empty()) {
        return Err("模型没有写回 ComfyUI 提示词".into());
    }
    Ok(out)
}

pub fn inject_prompt(workflow: &Value, prompt: &str, node_hint: &str) -> Result<Value, String> {
    let mut wf = workflow.clone();
    let obj = wf
        .as_object_mut()
        .ok_or_else(|| "ComfyUI 工作流须为 API JSON 对象（节点 id → 节点）".to_string())?;
    let target = pick_prompt_node(obj, node_hint)
        .ok_or_else(|| "工作流里找不到可写入提示词的节点（设 node id，或包含 text/prompt 的 MiniMax / CLIP 节点）".to_string())?;
    let node = obj
        .get_mut(&target)
        .and_then(|n| n.as_object_mut())
        .ok_or_else(|| "提示词节点无效".to_string())?;
    let inputs = node
        .entry("inputs")
        .or_insert_with(|| json!({}));
    let inputs = inputs
        .as_object_mut()
        .ok_or_else(|| "节点 inputs 不是对象".to_string())?;
    let key = if inputs.contains_key("prompt") {
        "prompt"
    } else if inputs.contains_key("text") {
        "text"
    } else if inputs.contains_key("prompts") {
        "prompts"
    } else {
        "text"
    };
    inputs.insert(key.into(), json!(prompt));
    Ok(wf)
}

pub fn is_chain_workflow(workflow: &Value, node_hint: &str) -> bool {
    let Some(obj) = workflow.as_object() else {
        return false;
    };
    if let Some(id) = pick_prompt_node(obj, node_hint) {
        if let Some(cls) = obj.get(&id).and_then(|n| n.get("class_type")).and_then(|v| v.as_str()) {
            let c = cls.to_ascii_lowercase();
            if c.contains("chain") || c.contains("shot") && c.contains("list") {
                return true;
            }
        }
        if obj
            .get(&id)
            .and_then(|n| n.get("inputs"))
            .and_then(|i| i.get("prompts"))
            .is_some()
        {
            return true;
        }
    }
    false
}

pub fn join_shot_prompts(shots: &[ChapterShot]) -> String {
    shots
        .iter()
        .map(|s| {
            let body = if s.comfy_prompt.trim().is_empty() {
                fallback_prompt(s)
            } else {
                s.comfy_prompt.trim().to_string()
            };
            format!("[dur={}]\n{body}", s.duration_sec)
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

pub fn fallback_prompt(s: &ChapterShot) -> String {
    let mut lines = vec![s.action.trim().to_string()];
    if !s.camera.trim().is_empty() {
        lines.push(format!("Camera: {}", s.camera.trim()));
    }
    if !s.dialogue.trim().is_empty() {
        lines.push(format!("Dialogue: {}", s.dialogue.trim()));
    }
    lines.join("\n")
}

pub fn normalize_comfy_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

pub async fn probe_comfy(url: &str) -> Result<(), String> {
    let base = normalize_comfy_url(url);
    if base.is_empty() {
        return Err("请先在设置填写 ComfyUI Desktop 地址".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(format!("{base}/system_stats"))
        .send()
        .await
        .map_err(|e| format!("连不上 ComfyUI（{base}）：{e}"))?;
    if !res.status().is_success() {
        return Err(format!("ComfyUI 无响应：HTTP {}", res.status()));
    }
    Ok(())
}

pub async fn queue_prompt(url: &str, workflow: &Value) -> Result<String, String> {
    let base = normalize_comfy_url(url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let body = json!({
        "prompt": workflow,
        "client_id": Uuid::new_v4().to_string(),
    });
    let res = client
        .post(format!("{base}/prompt"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("提交 ComfyUI 失败：{e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("ComfyUI 拒绝工作流（{status}）：{}", text.chars().take(400).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    v.get("prompt_id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("ComfyUI 未返回 prompt_id：{}", text.chars().take(200).collect::<String>()))
}

pub async fn submit_shots(
    url: &str,
    workflow: &Value,
    node_hint: &str,
    shots: &[ChapterShot],
) -> Result<ComfySubmitResult, String> {
    if shots.is_empty() {
        return Err("没有分镜头可提交".into());
    }
    if workflow.as_object().is_none() {
        return Err("请先在设置粘贴 ComfyUI「导出 API」工作流 JSON".into());
    }
    probe_comfy(url).await?;
    let mut prompt_ids = Vec::new();
    let mode = if is_chain_workflow(workflow, node_hint) {
        let wf = inject_prompt(workflow, &join_shot_prompts(shots), node_hint)?;
        prompt_ids.push(queue_prompt(url, &wf).await?);
        "chain".to_string()
    } else {
        for s in shots {
            let text = if s.comfy_prompt.trim().is_empty() {
                fallback_prompt(s)
            } else {
                s.comfy_prompt.clone()
            };
            let wf = inject_prompt(workflow, &text, node_hint)?;
            prompt_ids.push(queue_prompt(url, &wf).await?);
        }
        "each".to_string()
    };
    Ok(ComfySubmitResult {
        queued: prompt_ids.len(),
        prompt_ids,
        mode,
        url: normalize_comfy_url(url),
    })
}

#[derive(Debug, Clone)]
pub struct ComfyImageRef {
    pub filename: String,
    pub subfolder: String,
    pub typ: String,
}

pub async fn wait_for_output_images(
    url: &str,
    prompt_id: &str,
    timeout: std::time::Duration,
) -> Result<Vec<ComfyImageRef>, String> {
    let base = normalize_comfy_url(url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err("ComfyUI 出图超时，请到 ComfyUI 窗口查看队列".into());
        }
        let res = client
            .get(format!("{base}/history/{prompt_id}"))
            .send()
            .await
            .map_err(|e| format!("读取 ComfyUI 进度失败：{e}"))?;
        if res.status().is_success() {
            let v: Value = res.json().await.map_err(|e| e.to_string())?;
            let entry = v.get(prompt_id).cloned().unwrap_or(Value::Null);
            if let Some(status) = entry.get("status") {
                let err = status
                    .get("status_str")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if err.eq_ignore_ascii_case("error") {
                    return Err("ComfyUI 工作流执行失败".into());
                }
            }
            let mut imgs = Vec::new();
            if let Some(outputs) = entry.get("outputs").and_then(|x| x.as_object()) {
                for node in outputs.values() {
                    if let Some(arr) = node.get("images").and_then(|x| x.as_array()) {
                        for img in arr {
                            let filename = img
                                .get("filename")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            if filename.is_empty() {
                                continue;
                            }
                            imgs.push(ComfyImageRef {
                                filename,
                                subfolder: img
                                    .get("subfolder")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                typ: img
                                    .get("type")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("output")
                                    .to_string(),
                            });
                        }
                    }
                }
            }
            if !imgs.is_empty() {
                return Ok(imgs);
            }
            let done = entry
                .get("status")
                .and_then(|s| s.get("completed"))
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            if done {
                return Err("ComfyUI 已结束，但工作流没有输出图片。请确认设置里贴的是文生图 API 工作流".into());
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

pub async fn download_view(url: &str, img: &ComfyImageRef) -> Result<Vec<u8>, String> {
    let base = normalize_comfy_url(url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(format!("{base}/view"))
        .query(&[
            ("filename", img.filename.as_str()),
            ("subfolder", img.subfolder.as_str()),
            ("type", img.typ.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("下载 ComfyUI 图片失败：{e}"))?;
    if !res.status().is_success() {
        return Err(format!("下载 ComfyUI 图片失败：HTTP {}", res.status()));
    }
    res.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

fn pick_prompt_node(
    obj: &serde_json::Map<String, Value>,
    hint: &str,
) -> Option<String> {
    let hint = hint.trim();
    if !hint.is_empty() && obj.contains_key(hint) {
        return Some(hint.to_string());
    }
    let mut scored: Vec<(i32, String)> = Vec::new();
    for (id, node) in obj {
        let cls = node
            .get("class_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let inputs = node.get("inputs").and_then(|v| v.as_object());
        let has_text = inputs
            .map(|i| i.contains_key("text") || i.contains_key("prompt") || i.contains_key("prompts"))
            .unwrap_or(false);
        if !has_text {
            continue;
        }
        let mut score = 1;
        if cls.contains("minimax") {
            score += 8;
        }
        if cls.contains("chain") || cls.contains("promptor") {
            score += 6;
        }
        if cls.contains("cliptext") || cls.contains("textencode") {
            score += 3;
        }
        scored.push((score, id.clone()));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, id)| id).next()
}

fn first_shots_array(text: &str) -> Option<Vec<Value>> {
    for v in extract_objects(text) {
        if let Some(arr) = v.get("shots").and_then(|x| x.as_array()) {
            return Some(arr.clone());
        }
    }
    None
}

fn extract_objects(text: &str) -> Vec<Value> {
    let s = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return vec![v];
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            i += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut end = None;
        for (j, c) in chars.iter().enumerate().skip(i) {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = end {
            let slice: String = chars[i..=end].iter().collect();
            if let Ok(v) = serde_json::from_str(&slice) {
                out.push(v);
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shots_and_inject_minimax_text() {
        let shots = parse_shots_reply(
            r#"{"shots":[{"action":"推门","camera":"肩后","dialogue":"谁？","duration_sec":7}]}"#,
        )
        .unwrap();
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].duration_sec, 7);

        let wf = json!({
            "9": {"class_type":"CLIPTextEncode","inputs":{"text":"old"}},
            "12": {"class_type":"MiniMaxH3","inputs":{"prompt":"x","steps":6}}
        });
        let out = inject_prompt(&wf, "rain alley", "").unwrap();
        assert_eq!(out["12"]["inputs"]["prompt"], "rain alley");
        assert!(is_chain_workflow(
            &json!({"1":{"class_type":"H3AutoPromptChain","inputs":{"prompts":""}}}),
            ""
        ));
    }
}
