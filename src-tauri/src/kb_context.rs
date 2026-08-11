//! Context budgets + knowledge retrieval for writing/outline prompts.
//! Keyword always works; embeddings via OpenAI-compat when a provider key exists.
use crate::chapter_memory;
use crate::db::Db;
use crate::models::AppSettings;
use anyhow::{anyhow, Result};
use serde::Deserialize;

/// Per knowledge-card extract cap (chars).
pub const KNOWLEDGE_CARD_EXTRACT_CAP: usize = 500;
/// Root-linked knowledge cards total budget.
pub const ROOT_KNOWLEDGE_TOTAL_CAP: usize = 2000;
/// from_canon setting card cap.
pub const FROM_CANON_CARD_CAP: usize = 800;
/// Canon retrieval: top-k chunks.
pub const CANON_RETRIEVE_K: usize = 5;
/// Recall pool before rule rerank (also at least `k * 4`).
pub const CANON_RECALL_N: usize = 20;
/// Canon retrieval: chars per chunk.
pub const CANON_CHUNK_CAP: usize = 400;
/// Hard cap for entire canon block.
pub const CANON_BLOCK_CAP: usize = 3000;
/// Chapter memory facts injected into refine/outline.
pub const MEMORY_RETRIEVE_K: usize = 6;
pub const MEMORY_FACT_CAP: usize = 300;
pub const MEMORY_BLOCK_CAP: usize = 2000;
/// Knowledge cards total in chapter_context.
pub const CHAPTER_KNOWLEDGE_TOTAL_CAP: usize = 2000;

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
    // Embedding boost when vectors exist (recall stage only)
    if let Ok(Some(qvec)) = embed_query_blocking(db, query) {
        for (id, _, _) in &rows {
            let Some((bid, idx_s)) = id.split_once('#') else {
                continue;
            };
            let Ok(idx) = idx_s.parse::<i64>() else {
                continue;
            };
            if let Ok(Some(vec)) = db.get_knowledge_embedding(bid, idx) {
                let sim = cosine(&qvec, &vec);
                if sim > 0.15 {
                    let bonus = (sim * 20.0) as usize;
                    *recall_bonus.entry(id.clone()).or_default() += bonus.max(1);
                }
            }
        }
    }

    retrieve_knowledge_from_rows(&rows, query, k, chunk_cap, Some(&recall_bonus))
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

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let d = na.sqrt() * nb.sqrt();
    if d < 1e-8 {
        0.0
    } else {
        dot / d
    }
}

/// Sync embed for retrieve path (spawn_blocking not needed — small query).
fn embed_query_blocking(db: &Db, query: &str) -> Result<Option<Vec<f32>>> {
    let settings = db.get_settings()?;
    let rt = tokio::runtime::Handle::try_current();
    match rt {
        Ok(handle) => {
            // We're likely already on async runtime inside tauri; block_in_place
            tokio::task::block_in_place(|| {
                handle.block_on(async { embed_texts(&settings, &[query.to_string()]).await })
            })
            .map(|v| v.into_iter().next())
        }
        Err(_) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async { embed_texts(&settings, &[query.to_string()]).await })
                .map(|v| v.into_iter().next())
        }
    }
}

fn embeddings_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/embeddings")
    } else {
        format!("{b}/v1/embeddings")
    }
}

/// OpenAI-compat embeddings; uses first compat provider with a key.
pub async fn embed_texts(settings: &AppSettings, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let p = settings
        .compat_providers
        .iter()
        .find(|p| !p.api_key.trim().is_empty() && p.protocol == "openai")
        .ok_or_else(|| anyhow!("no openai compat provider for embeddings"))?;
    if texts.is_empty() {
        return Ok(vec![]);
    }
    #[derive(Deserialize)]
    struct EmbResp {
        data: Vec<EmbItem>,
    }
    #[derive(Deserialize)]
    struct EmbItem {
        embedding: Vec<f32>,
        index: usize,
    }
    // Common OpenAI-compat embedding model ids; providers may alias.
    let model = "text-embedding-3-small";
    let body = serde_json::json!({
        "model": model,
        "input": texts,
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(embeddings_url(&p.base_url))
        .bearer_auth(&p.api_key)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("embeddings {status}: {text}"));
    }
    let parsed: EmbResp = resp.json().await?;
    let mut out = vec![Vec::new(); texts.len()];
    for item in parsed.data {
        if item.index < out.len() {
            out[item.index] = item.embedding;
        }
    }
    if out.iter().any(|v| v.is_empty()) {
        return Err(anyhow!("incomplete embedding response"));
    }
    Ok(out)
}

/// Index all chunks for a book (best-effort).
pub async fn index_book_embeddings(db: &Db, settings: &AppSettings, book_id: &str) -> Result<usize> {
    let chunks = db.list_knowledge_chunks(book_id)?;
    if chunks.is_empty() {
        return Ok(0);
    }
    // Batch to avoid huge payloads
    let mut n = 0;
    for batch in chunks.chunks(16) {
        let texts: Vec<String> = batch.iter().map(|c| c.content.clone()).collect();
        let vectors = match embed_texts(settings, &texts).await {
            Ok(v) => v,
            Err(_) => return Ok(n),
        };
        for (c, vec) in batch.iter().zip(vectors.into_iter()) {
            db.upsert_knowledge_embedding(book_id, c.idx as i64, &vec)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Cap a list of knowledge card bodies into a single string under total budget.
pub fn format_knowledge_cards_capped(
    cards: &[(String, String)],
    per_card: usize,
    total: usize,
    zh: bool,
) -> String {
    let mut parts = Vec::new();
    let mut used = 0usize;
    for (label, feat) in cards {
        let snip = truncate_chars(feat.trim(), per_card);
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
