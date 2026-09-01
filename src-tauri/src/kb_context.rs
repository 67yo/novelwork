//! Context budgets + knowledge retrieval for writing/outline prompts.
//! Keyword always works; knowledge embeddings via local MiniLM
//! (`paraphrase-multilingual-MiniLM-L12-v2` / fastembed).
use crate::chapter_memory;
use crate::db::Db;
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Per knowledge-card extract cap (chars).
pub const KNOWLEDGE_CARD_EXTRACT_CAP: usize = 500;
/// Root-linked knowledge cards total budget.
pub const ROOT_KNOWLEDGE_TOTAL_CAP: usize = 2000;
/// Knowledge retrieval: top-k chunks (`search_knowledge`).
pub const CANON_RETRIEVE_K: usize = 5;
/// Recall pool before rule rerank (also at least `k * 4`).
pub const CANON_RECALL_N: usize = 20;
/// Knowledge retrieval: chars per chunk.
pub const CANON_CHUNK_CAP: usize = 400;
/// Chapter memory facts injected into refine/outline.
pub const MEMORY_RETRIEVE_K: usize = 6;
pub const MEMORY_FACT_CAP: usize = 300;
pub const MEMORY_BLOCK_CAP: usize = 2000;
/// Knowledge cards total in chapter_context.
pub const CHAPTER_KNOWLEDGE_TOTAL_CAP: usize = 2000;
/// Local MiniLM output width (`paraphrase-multilingual-MiniLM-L12-v2`).
pub const MINILM_DIMS: usize = 384;

pub fn truncate_chars(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// True when client pasted a full assembled outline brief (legacy).
pub fn looks_like_assembled_brief(s: &str) -> bool {
    let t = s.trim();
    if t.chars().count() > 1200 {
        return true;
    }
    t.contains("【须考虑")
        || t.contains("[Materials")
        || t.contains("须考虑的材料")
        || t.contains("Materials & expectations")
        || t.contains("【根参考")
        || t.contains("[Root reference")
        || t.contains("## 根节点与全书设定")
        || t.contains("## Root & novel setup")
        || t.contains("## 当前章大纲")
        || t.contains("## Current chapter outline")
}

/// If `user_brief` is empty or short expectation-only, assemble materials and append expectation.
pub fn merge_expectation_into_assembled(assembled: String, user_brief: &str, zh: bool) -> String {
    let expect = user_brief.trim();
    if expect.is_empty() || looks_like_assembled_brief(expect) {
        if expect.is_empty() {
            assembled
        } else {
            expect.to_string()
        }
    } else if zh {
        format!("{assembled}\n\n【用户对本批的期望】\n{expect}\n")
    } else {
        format!("{assembled}\n\n[User expectations for this batch]\n{expect}\n")
    }
}

/// Keyword (+ optional embedding) retrieval over bound books.
/// Returns `(source_id, title_or_label, preview)` with source_id = `book_id#idx`.
pub fn retrieve_knowledge(
    db: &Db,
    book_ids: &[String],
    query: &str,
    k: usize,
    chunk_cap: usize,
) -> Vec<(String, String, String)> {
    if book_ids.is_empty() || k == 0 {
        return Vec::new();
    }
    let mut rows: Vec<(String, String, String)> = Vec::new(); // id, title, content
    for bid in book_ids {
        let title = db
            .get_knowledge(bid)
            .ok()
            .flatten()
            .map(|b| b.title)
            .unwrap_or_else(|| bid.clone());
        if let Ok(chunks) = db.list_knowledge_chunks(bid) {
            for c in chunks {
                let sid = format!("{bid}#{}", c.idx);
                rows.push((sid, title.clone(), c.content));
            }
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }

    let mut recall_bonus: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    if let Ok(bonus) = sqlite_vec_recall_bonus(book_ids, query, recall_pool_size(k)) {
        recall_bonus = bonus;
    }

    retrieve_knowledge_from_rows(&rows, query, k, chunk_cap, Some(&recall_bonus))
}

/// Concatenate public-library chunks for a knowledge card “full import”.
/// Prefers analysis / TOC chunks; stops at `max_chars`.
pub fn gather_knowledge_corpus(
    db: &Db,
    book_ids: &[String],
    max_chars: usize,
) -> Result<String> {
    let mut out = String::new();
    for bid in book_ids {
        let book = db
            .get_knowledge(bid)
            .map_err(|e| anyhow!("{e}"))?
            .ok_or_else(|| anyhow!("知识库不存在: {bid}"))?;
        if book.archived {
            continue;
        }
        let chunks = db
            .list_knowledge_chunks(bid)
            .map_err(|e| anyhow!("{e}"))?;
        out.push_str(&format!("### 《{}》· {}\n", book.title, book.author));
        let mut ordered = chunks;
        ordered.sort_by_key(|c| {
            let p = if c.content.contains("【知识库分析】") {
                0
            } else if c.content.contains("【目录】") {
                1
            } else {
                2
            };
            (p, c.idx)
        });
        for c in ordered.into_iter().take(24) {
            let piece = format!("{}\n\n", c.content);
            if out.chars().count() + piece.chars().count() > max_chars {
                return Ok(out);
            }
            out.push_str(&piece);
        }
    }
    Ok(out)
}

fn recall_pool_size(k: usize) -> usize {
    CANON_RECALL_N.max(k.saturating_mul(4)).max(k)
}

/// Pure retrieve path (no DB): keyword recall → rule rerank → top-k.
/// `rows`: `(source_id, book_title, full_content)`.
/// `recall_bonus`: optional per-id score added at recall (e.g. embedding).
pub fn retrieve_knowledge_from_rows(
    rows: &[(String, String, String)],
    query: &str,
    k: usize,
    chunk_cap: usize,
    recall_bonus: Option<&std::collections::HashMap<String, usize>>,
) -> Vec<(String, String, String)> {
    if rows.is_empty() || k == 0 {
        return Vec::new();
    }
    let pool = recall_pool_size(k);
    let kw_rows: Vec<(String, String)> = rows
        .iter()
        .map(|(id, _, c)| (id.clone(), c.clone()))
        .collect();

    let mut recall_score: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (id, _) in chapter_memory::rank_memory(&kw_rows, query, pool) {
        *recall_score.entry(id).or_default() += 10;
    }
    if let Some(bonus) = recall_bonus {
        for (id, b) in bonus {
            if *b > 0 {
                *recall_score.entry(id.clone()).or_default() += *b;
            }
        }
    }

    let mut recall_ranked: Vec<_> = recall_score.into_iter().collect();
    recall_ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut candidates: Vec<(String, String, String)> = Vec::new();
    for (id, _) in recall_ranked.into_iter().take(pool) {
        if let Some((sid, title, content)) = rows.iter().find(|(s, _, _)| *s == id) {
            candidates.push((sid.clone(), title.clone(), content.clone()));
        }
    }
    // Fallback: keyword-only if recall empty
    if candidates.is_empty() {
        for (id, c) in chapter_memory::rank_memory(&kw_rows, query, k) {
            if let Some((_, title, content)) = rows.iter().find(|(sid, _, _)| *sid == id) {
                candidates.push((id, title.clone(), content.clone()));
            } else {
                candidates.push((id, String::new(), c));
            }
        }
        return candidates
            .into_iter()
            .take(k)
            .map(|(id, title, content)| (id, title, truncate_chars(&content, chunk_cap)))
            .collect();
    }

    let reranked = rerank_knowledge_candidates(query, &candidates);
    if reranked.iter().all(|(_, s)| *s == 0) {
        // Scores all zero → keep recall order (not worse than today)
        return candidates
            .into_iter()
            .take(k)
            .map(|(id, title, content)| (id, title, truncate_chars(&content, chunk_cap)))
            .collect();
    }

    reranked
        .into_iter()
        .take(k)
        .filter_map(|(id, _)| {
            candidates
                .iter()
                .find(|(sid, _, _)| *sid == id)
                .map(|(sid, title, content)| {
                    (
                        sid.clone(),
                        title.clone(),
                        truncate_chars(content, chunk_cap),
                    )
                })
        })
        .collect()
}

/// Rule rerank over recall candidates. Returns `(source_id, score)` desc.
/// ponytail: local term/title boost only; upgrade to LLM/cross-encoder if precision stalls.
pub fn rerank_knowledge_candidates(
    query: &str,
    candidates: &[(String, String, String)],
) -> Vec<(String, usize)> {
    let terms = chapter_memory::query_terms(query);
    let mut scored: Vec<(String, usize)> = candidates
        .iter()
        .map(|(id, _title, content)| {
            let mut score = chapter_memory::score_memory(content, &terms);
            // Proper-noun-ish terms (len >= 2 already): extra weight per hit
            let hay = content.to_lowercase();
            for t in &terms {
                if t.chars().count() >= 2 && hay.contains(t.as_str()) {
                    score += 1;
                    if t.chars().count() >= 3 {
                        score += 1;
                    }
                }
            }
            // Chapter / first-line title boost
            let head = content.lines().next().unwrap_or("").to_lowercase();
            let is_chapter_head = head.contains('章')
                || head.starts_with('【')
                || head.starts_with('#');
            if is_chapter_head {
                for t in &terms {
                    if !t.is_empty() && head.contains(t.as_str()) {
                        score += 4;
                    }
                }
            }
            (id.clone(), score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored
}

fn sqlite_vec_recall_bonus(
    book_ids: &[String],
    query: &str,
    k: usize,
) -> Result<std::collections::HashMap<String, usize>> {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return Ok(Default::default());
    };
    tokio::task::block_in_place(|| {
        handle.block_on(crate::knowledge_vec::recall_bonus(book_ids, query, k))
    })
}

static LOCAL_EMBEDDER: Lazy<Mutex<Option<fastembed::TextEmbedding>>> =
    Lazy::new(|| Mutex::new(None));

fn emit_kb_model_progress(app: Option<&tauri::AppHandle>, payload: serde_json::Value) {
    if let Some(app) = app {
        use tauri::Emitter;
        let _ = app.emit("knowledge-model-progress", payload);
    }
}

/// HF hubs that serve full files without requiring Content-Range (hf-hub Range GET breaks on many mirrors/CDNs).
fn hf_base_urls() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(ep) = std::env::var("HF_ENDPOINT") {
        let ep = ep.trim().trim_end_matches('/').to_string();
        if !ep.is_empty() {
            out.push(ep);
        }
    }
    for b in ["https://huggingface.co", "https://hf-mirror.com"] {
        if !out.iter().any(|x| x == b) {
            out.push(b.to_string());
        }
    }
    out
}

const MINILM_REPO: &str = "Xenova/paraphrase-multilingual-MiniLM-L12-v2";
const MINILM_FILES: &[&str] = &[
    "onnx/model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

/// Approximate share of total download (onnx dominates).
fn minilm_file_weight(rel: &str) -> f64 {
    if rel.ends_with("model.onnx") {
        0.90
    } else {
        0.10 / (MINILM_FILES.len().saturating_sub(1).max(1) as f64)
    }
}

fn minilm_model_dir() -> std::path::PathBuf {
    let p = crate::paths::embed_models_dir().join("paraphrase-multilingual-MiniLM-L12-v2");
    std::fs::create_dir_all(&p).ok();
    p
}

fn download_url_to_file(
    url: &str,
    dest: &std::path::Path,
    mut on_bytes: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    use std::io::Read;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("download");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| anyhow!("http client: {e}"))?;
    // Full GET only — no Range header (avoids "Header Content-Range is missing").
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| anyhow!("GET {url}: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("GET {url}: {e}"))?;
    let total = resp.content_length();
    let mut file = std::fs::File::create(&tmp).map_err(|e| anyhow!("create {tmp:?}: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last_emit = 0u64;
    on_bytes(0, total);
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| anyhow!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        use std::io::Write;
        file.write_all(&buf[..n])
            .map_err(|e| anyhow!("write {tmp:?}: {e}"))?;
        done += n as u64;
        let step = total.map(|t| (t / 50).max(256 * 1024)).unwrap_or(512 * 1024);
        if done - last_emit >= step || total == Some(done) {
            last_emit = done;
            on_bytes(done, total);
        }
    }
    on_bytes(done, total.or(Some(done)));
    file.sync_all().ok();
    drop(file);
    std::fs::rename(&tmp, dest).map_err(|e| anyhow!("rename to {dest:?}: {e}"))?;
    Ok(())
}

fn ensure_minilm_file(
    rel: &str,
    app: Option<&tauri::AppHandle>,
    file_index: usize,
    weight_before: f64,
    weight: f64,
) -> Result<std::path::PathBuf> {
    let dest = minilm_model_dir().join(rel);
    let file_total = MINILM_FILES.len();
    if dest.is_file() {
        let len = dest.metadata().map(|m| m.len()).unwrap_or(0);
        if len > 0 {
            emit_kb_model_progress(
                app,
                serde_json::json!({
                    "phase": "download",
                    "file": rel,
                    "fileIndex": file_index,
                    "fileTotal": file_total,
                    "bytesDownloaded": len,
                    "bytesTotal": len,
                    "percent": ((weight_before + weight) * 90.0).round() as u32,
                }),
            );
            return Ok(dest);
        }
    }
    let mut last = None;
    for base in hf_base_urls() {
        let url = format!("{base}/{MINILM_REPO}/resolve/main/{rel}");
        let app_ref = app;
        match download_url_to_file(&url, &dest, |done, total| {
            let frac = match total {
                Some(t) if t > 0 => (done as f64 / t as f64).clamp(0.0, 1.0),
                _ => 0.0,
            };
            let percent = ((weight_before + weight * frac) * 90.0).round() as u32;
            emit_kb_model_progress(
                app_ref,
                serde_json::json!({
                    "phase": "download",
                    "file": rel,
                    "fileIndex": file_index,
                    "fileTotal": file_total,
                    "bytesDownloaded": done,
                    "bytesTotal": total,
                    "percent": percent.min(90),
                }),
            );
        }) {
            Ok(()) => return Ok(dest),
            Err(e) => {
                let _ = std::fs::remove_file(dest.with_extension("download"));
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("无法下载 {rel}")))
}

fn load_local_minilm(app: Option<&tauri::AppHandle>) -> Result<fastembed::TextEmbedding> {
    let mut weight_before = 0.0;
    for (i, rel) in MINILM_FILES.iter().enumerate() {
        let w = minilm_file_weight(rel);
        ensure_minilm_file(rel, app, i + 1, weight_before, w)?;
        weight_before += w;
    }
    emit_kb_model_progress(
        app,
        serde_json::json!({
            "phase": "load",
            "file": "onnx/model.onnx",
            "fileIndex": MINILM_FILES.len(),
            "fileTotal": MINILM_FILES.len(),
            "percent": 92,
        }),
    );
    let dir = minilm_model_dir();
    let onnx =
        std::fs::read(dir.join("onnx/model.onnx")).map_err(|e| anyhow!("read onnx: {e}"))?;
    let tokenizer_files = fastembed::TokenizerFiles {
        tokenizer_file: std::fs::read(dir.join("tokenizer.json"))
            .map_err(|e| anyhow!("read tokenizer.json: {e}"))?,
        config_file: std::fs::read(dir.join("config.json"))
            .map_err(|e| anyhow!("read config.json: {e}"))?,
        special_tokens_map_file: std::fs::read(dir.join("special_tokens_map.json"))
            .map_err(|e| anyhow!("read special_tokens_map.json: {e}"))?,
        tokenizer_config_file: std::fs::read(dir.join("tokenizer_config.json"))
            .map_err(|e| anyhow!("read tokenizer_config.json: {e}"))?,
    };
    let model = fastembed::UserDefinedEmbeddingModel::new(onnx, tokenizer_files)
        .with_pooling(fastembed::Pooling::Mean);
    let emb = fastembed::TextEmbedding::try_new_from_user_defined(
        model,
        fastembed::InitOptionsUserDefined::new(),
    )
    .map_err(|e| anyhow!("load MiniLM onnx: {e}"))?;
    emit_kb_model_progress(
        app,
        serde_json::json!({
            "phase": "ready",
            "percent": 95,
        }),
    );
    Ok(emb)
}

fn with_local_embedder<R>(
    app: Option<&tauri::AppHandle>,
    f: impl FnOnce(&mut fastembed::TextEmbedding) -> Result<R>,
) -> Result<R> {
    let mut guard = LOCAL_EMBEDDER.lock();
    if guard.is_none() {
        // 自管下载到 app data（整文件 GET），绕过 hf-hub 的 Range/Content-Range 问题
        let model = load_local_minilm(app).map_err(|e| {
            anyhow!(
                "init local MiniLM: {e}\n\
                 模型会缓存到 {:?}。可设置 HF_ENDPOINT（如 https://hf-mirror.com）后重试。",
                minilm_model_dir()
            )
        })?;
        *guard = Some(model);
    }
    f(guard.as_mut().expect("embedder just initialized"))
}

fn embed_texts_blocking(
    texts: &[String],
    app: Option<&tauri::AppHandle>,
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    let docs = texts.to_vec();
    with_local_embedder(app, |model| {
        model
            .embed(docs, None)
            .map_err(|e| anyhow!("local embed: {e}"))
    })
}

/// Local MiniLM embeddings (paraphrase-multilingual-MiniLM-L12-v2).
pub async fn embed_texts(
    texts: &[String],
    app: Option<tauri::AppHandle>,
) -> Result<Vec<Vec<f32>>> {
    let texts = texts.to_vec();
    tokio::task::spawn_blocking(move || embed_texts_blocking(&texts, app.as_ref()))
        .await
        .map_err(|e| anyhow!("embed join: {e}"))?
}

/// rig `EmbeddingModel` over the local MiniLM (no `rig-fastembed` / ort clash).
#[derive(Clone, Default)]
pub struct MiniLmEmbedding;

impl MiniLmEmbedding {
    pub fn new() -> Self {
        Self
    }
}

impl rig_core::embeddings::EmbeddingModel for MiniLmEmbedding {
    const MAX_DOCUMENTS: usize = 16;
    type Client = ();

    fn make(_client: &Self::Client, _model: impl Into<String>, _dims: Option<usize>) -> Self {
        Self
    }

    fn ndims(&self) -> usize {
        MINILM_DIMS
    }

    fn embed_texts(
        &self,
        texts: impl IntoIterator<Item = String> + rig_core::wasm_compat::WasmCompatSend,
    ) -> impl std::future::Future<
        Output = Result<Vec<rig_core::embeddings::Embedding>, rig_core::embeddings::EmbeddingError>,
    > + rig_core::wasm_compat::WasmCompatSend {
        let texts: Vec<String> = texts.into_iter().collect();
        async move {
            let t2 = texts.clone();
            let vectors = tokio::task::spawn_blocking(move || embed_texts_blocking(&t2, None))
                .await
                .map_err(|e| {
                    rig_core::embeddings::EmbeddingError::ProviderError(e.to_string())
                })?
                .map_err(|e| {
                    rig_core::embeddings::EmbeddingError::ProviderError(e.to_string())
                })?;
            Ok(texts
                .into_iter()
                .zip(vectors)
                .map(|(document, vec)| rig_core::embeddings::Embedding {
                    document,
                    vec: vec.iter().copied().map(f64::from).collect(),
                })
                .collect())
        }
    }
}

/// Index all chunks for a book with local MiniLM into `rig-sqlite`.
pub async fn index_book_embeddings(
    db: &Db,
    book_id: &str,
    app: Option<tauri::AppHandle>,
) -> Result<usize> {
    let chunks = db.list_knowledge_chunks(book_id)?;
    if chunks.is_empty() {
        crate::knowledge_vec::delete_book(book_id).await.ok();
        return Ok(0);
    }
    let total = chunks.len();
    let pairs: Vec<(u32, String)> = chunks
        .iter()
        .map(|c| (c.idx, c.content.clone()))
        .collect();
    let mut all_vecs = Vec::with_capacity(total);
    let mut n = 0;
    for batch in chunks.chunks(16) {
        let texts: Vec<String> = batch.iter().map(|c| c.content.clone()).collect();
        let vectors = embed_texts(&texts, app.clone()).await?;
        n += vectors.len();
        all_vecs.extend(vectors);
        let percent = 95 + ((n as f64 / total as f64) * 5.0).round() as u32;
        emit_kb_model_progress(
            app.as_ref(),
            serde_json::json!({
                "phase": "embed",
                "done": n,
                "total": total,
                "percent": percent.min(100),
            }),
        );
    }
    crate::knowledge_vec::index_book(book_id, &pairs, &all_vecs).await?;
    emit_kb_model_progress(
        app.as_ref(),
        serde_json::json!({
            "phase": "done",
            "done": n,
            "total": total,
            "percent": 100,
        }),
    );
    Ok(n)
}

/// Cap a list of knowledge card bodies into a single string under total budget.
/// `cards`: (label, body, per_card_cap)
pub fn format_knowledge_cards_capped_var(
    cards: &[(String, String, usize)],
    total: usize,
    zh: bool,
) -> String {
    let mut parts = Vec::new();
    let mut used = 0usize;
    for (label, feat, per_card) in cards {
        let snip = truncate_chars(feat.trim(), *per_card);
        if snip.is_empty() {
            continue;
        }
        let block = if zh {
            format!("- 「{label}」\n{snip}")
        } else {
            format!("- “{label}”\n{snip}")
        };
        let add = block.chars().count() + 1;
        if used + add > total {
            break;
        }
        used += add;
        parts.push(block);
    }
    parts.join("\n")
}

/// Cap a list of knowledge card bodies into a single string under total budget.
pub fn format_knowledge_cards_capped(
    cards: &[(String, String)],
    per_card: usize,
    total: usize,
    zh: bool,
) -> String {
    let v: Vec<(String, String, usize)> = cards
        .iter()
        .map(|(l, b)| (l.clone(), b.clone(), per_card))
        .collect();
    format_knowledge_cards_capped_var(&v, total, zh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn truncate_and_merge_expectation() {
        assert_eq!(truncate_chars("abcd", 2), "ab…");
        let assembled = "【须考虑的材料】\nroot".to_string();
        let m = merge_expectation_into_assembled(assembled.clone(), "加快节奏", true);
        assert!(m.contains("加快节奏"));
        assert!(m.contains("须考虑"));
        let legacy = merge_expectation_into_assembled(assembled, &"x".repeat(1300), true);
        assert_eq!(legacy.chars().count(), 1300);
    }

    #[test]
    fn knowledge_cards_cap() {
        let cards = vec![
            ("A".into(), "a".repeat(800)),
            ("B".into(), "b".repeat(800)),
        ];
        let s = format_knowledge_cards_capped(&cards, 500, 600, true);
        assert!(s.chars().count() <= 600);
        assert!(s.contains('A'));
    }

    #[test]
    fn rerank_prefers_chapter_title_hit() {
        let query = "筑基禁忌";
        let candidates = vec![
            (
                "a#0".into(),
                "书".into(),
                "闲文里提一次禁忌，与筑基无关。".into(),
            ),
            (
                "a#1".into(),
                "书".into(),
                "【第三章 筑基禁忌】违者经脉逆乱。".into(),
            ),
        ];
        let ranked = rerank_knowledge_candidates(query, &candidates);
        assert_eq!(ranked[0].0, "a#1");
        assert!(ranked[0].1 > ranked[1].1);
    }

    #[derive(Deserialize)]
    struct FixtureCase {
        id: String,
        query: String,
        k: Option<usize>,
        chunks: Vec<FixtureChunk>,
        expect_source_ids: Vec<String>,
    }

    #[derive(Deserialize)]
    struct FixtureChunk {
        book_id: String,
        idx: u32,
        title: String,
        content: String,
    }

    #[test]
    fn kb_retrieve_fixture_cases() {
        let raw = include_str!("../tests/fixtures/kb_retrieve_cases.json");
        let cases: Vec<FixtureCase> =
            serde_json::from_str(raw).expect("parse kb_retrieve_cases.json");
        assert!(!cases.is_empty());
        for case in cases {
            let rows: Vec<(String, String, String)> = case
                .chunks
                .iter()
                .map(|c| {
                    (
                        format!("{}#{}", c.book_id, c.idx),
                        c.title.clone(),
                        c.content.clone(),
                    )
                })
                .collect();
            let k = case.k.unwrap_or(CANON_RETRIEVE_K);
            let hits =
                retrieve_knowledge_from_rows(&rows, &case.query, k, CANON_CHUNK_CAP, None);
            let hit_ids: Vec<&str> = hits.iter().map(|(id, _, _)| id.as_str()).collect();
            let ok = case
                .expect_source_ids
                .iter()
                .any(|want| hit_ids.iter().any(|h| *h == want.as_str()));
            assert!(
                ok,
                "case {} query={:?} expect {:?} got {:?}",
                case.id, case.query, case.expect_source_ids, hit_ids
            );
        }
    }
}
