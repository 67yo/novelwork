//! Per-novel chapter memory. Isolated by `novel_id`.
//! Listing lives in SQLite; vector search goes through `rig-lancedb`.

use crate::chunk::chunk_text;
use crate::kb_context::{embed_texts, MiniLmEmbedding, MINILM_DIMS};
use crate::paths::lancedb_dir;
use anyhow::Result;
use arrow_array::{
    types::Float64Type, ArrayRef, FixedSizeListArray, Int32Array, RecordBatch,
    RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use rig_core::vector_store::request::{SearchFilter, VectorSearchRequestBuilder};
use rig_core::vector_store::VectorStoreIndex;
use rig_lancedb::{LanceDBFilter, LanceDbVectorIndex, SearchParams, SearchType};
use serde::Deserialize;
use std::sync::Arc;

const TABLE: &str = "chapter_memory_vec";

#[derive(Debug, Deserialize)]
struct MemoryRow {
    node_id: String,
    content: String,
}

fn memory_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("novel_id", DataType::Utf8, false),
        Field::new("node_id", DataType::Utf8, false),
        Field::new("idx", DataType::Int32, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float64, true)),
                MINILM_DIMS as i32,
            ),
            false,
        ),
    ])
}

async fn open_db() -> Result<lancedb::Connection> {
    Ok(lancedb::connect(lancedb_dir().to_str().unwrap_or("./lancedb"))
        .execute()
        .await?)
}

/// Persist chapter fact chunks into LanceDB with MiniLM vectors (`rig-lancedb` search).
pub async fn upsert_lance(novel_id: &str, node_id: &str, chunks: &[String]) -> Result<()> {
    if chunks.is_empty() {
        return Ok(());
    }
    let _ = delete_lance(novel_id, node_id).await;

    let vectors = embed_texts(chunks, None).await?;
    let schema = Arc::new(memory_schema());
    let ids: StringArray = (0..chunks.len())
        .map(|i| Some(format!("{novel_id}:{node_id}:{i}")))
        .collect();
    let novel_ids: StringArray = chunks.iter().map(|_| Some(novel_id)).collect();
    let node_ids: StringArray = chunks.iter().map(|_| Some(node_id)).collect();
    let idxs: Int32Array = (0..chunks.len() as i32).map(Some).collect();
    let contents: StringArray = chunks.iter().map(|c| Some(c.as_str())).collect();
    let embedding = FixedSizeListArray::from_iter_primitive::<Float64Type, _, _>(
        vectors.into_iter().map(|v| {
            Some(
                v.into_iter()
                    .map(|x| Some(f64::from(x)))
                    .collect::<Vec<_>>(),
            )
        }),
        MINILM_DIMS as i32,
    );

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(ids) as ArrayRef,
            Arc::new(novel_ids) as ArrayRef,
            Arc::new(node_ids) as ArrayRef,
            Arc::new(idxs) as ArrayRef,
            Arc::new(contents) as ArrayRef,
            Arc::new(embedding) as ArrayRef,
        ],
    )?;
    let batches = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema.clone());
    let reader: Box<dyn arrow_array::RecordBatchReader + Send> = Box::new(batches);

    let db = open_db().await?;
    let names = db.table_names().execute().await.unwrap_or_default();
    if names.iter().any(|n| n == TABLE) {
        let table = db.open_table(TABLE).execute().await?;
        table.add(reader).execute().await?;
    } else {
        db.create_table(TABLE, reader).execute().await?;
    }
    Ok(())
}

pub async fn delete_lance(novel_id: &str, node_id: &str) -> Result<()> {
    let db = open_db().await?;
    let names = db.table_names().execute().await.unwrap_or_default();
    if !names.iter().any(|n| n == TABLE) {
        return Ok(());
    }
    let table = db.open_table(TABLE).execute().await?;
    let n = novel_id.replace('\'', "''");
    let c = node_id.replace('\'', "''");
    let _ = table
        .delete(&format!("novel_id = '{n}' AND node_id = '{c}'"))
        .await;
    Ok(())
}

pub async fn delete_lance_novel(novel_id: &str) -> Result<()> {
    let db = open_db().await?;
    let names = db.table_names().execute().await.unwrap_or_default();
    if !names.iter().any(|n| n == TABLE) {
        return Ok(());
    }
    let table = db.open_table(TABLE).execute().await?;
    let n = novel_id.replace('\'', "''");
    let _ = table.delete(&format!("novel_id = '{n}'")).await;
    Ok(())
}

pub async fn search_vectors(
    novel_id: &str,
    query: &str,
    k: usize,
) -> Result<Vec<(f64, String, String)>> {
    if novel_id.is_empty() || query.trim().is_empty() || k == 0 {
        return Ok(Vec::new());
    }
    let db = open_db().await?;
    let names = db.table_names().execute().await.unwrap_or_default();
    if !names.iter().any(|n| n == TABLE) {
        return Ok(Vec::new());
    }
    let table = db.open_table(TABLE).execute().await?;
    let params = SearchParams::default()
        .distance_type(lancedb::DistanceType::Cosine)
        .search_type(SearchType::Flat);
    let index = LanceDbVectorIndex::new(table, MiniLmEmbedding::new(), "id", params).await?;
    let req = VectorSearchRequestBuilder::<LanceDBFilter>::default()
        .query(query)
        .samples(k as u64)
        .filter(LanceDBFilter::eq(
            "novel_id",
            serde_json::Value::String(novel_id.to_string()),
        ))
        .build();
    let hits = index.top_n::<MemoryRow>(req).await?;
    Ok(hits
        .into_iter()
        .map(|(score, _id, row)| (score, row.node_id, row.content))
        .collect())
}

fn is_section_header(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with('【') && t.contains('】') {
        return true;
    }
    if t.starts_with('#') {
        return true;
    }
    let lower = t.to_lowercase();
    lower.starts_with("[overall")
        || lower.starts_with("[facts")
        || lower == "[plot]"
        || t == "整体情节"
        || t == "要点"
}

fn is_bullet(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('-') || t.starts_with('•') || t.starts_with('*')
}

/// Split LLM extraction into storage chunks (plot summary + fact bullets).
pub fn split_memory_text(text: &str) -> Vec<String> {
    let t = text.trim();
    if t.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut plot_buf = String::new();
    let mut in_plot = false;

    let flush_plot = |buf: &mut String, out: &mut Vec<String>| {
        let s = buf.trim().to_string();
        buf.clear();
        if s.chars().count() >= 4 {
            if s.contains("整体情节") || s.to_lowercase().contains("overall plot") {
                out.push(s);
            } else {
                out.push(format!("【整体情节】\n{s}"));
            }
        }
    };

    for line in t.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_section_header(trimmed) {
            let lower = trimmed.to_lowercase();
            let plot_hdr = trimmed.contains("整体情节")
                || lower.contains("overall plot")
                || lower.contains("[overall");
            let facts_hdr = trimmed.contains("要点")
                || trimmed.contains("已交代")
                || lower.contains("[facts")
                || lower.contains("facts]")
                || lower.contains("already shown")
                || lower.contains("[revealed");
            if plot_hdr && !facts_hdr {
                if in_plot {
                    flush_plot(&mut plot_buf, &mut out);
                }
                in_plot = true;
                continue;
            }
            if facts_hdr || (in_plot && !plot_hdr) {
                if in_plot {
                    flush_plot(&mut plot_buf, &mut out);
                    in_plot = false;
                }
                continue;
            }
        }
        if in_plot {
            if is_bullet(trimmed) {
                flush_plot(&mut plot_buf, &mut out);
                in_plot = false;
            } else {
                if !plot_buf.is_empty() {
                    plot_buf.push('\n');
                }
                plot_buf.push_str(trimmed);
                continue;
            }
        }
        if is_bullet(trimmed) {
            let fact = trimmed
                .trim_start_matches(['-', '*', '•', ' '])
                .trim()
                .to_string();
            if fact.chars().count() >= 4 {
                out.push(fact);
            }
        }
    }
    if in_plot {
        flush_plot(&mut plot_buf, &mut out);
    }

    if out.len() >= 1 {
        return out;
    }
    // Fallback: old-style bullet-only or free text
    let bullets: Vec<String> = t
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && is_bullet(l))
        .map(|l| l.trim_start_matches(['-', '*', '•', ' ']).trim().to_string())
        .filter(|l| l.chars().count() >= 4)
        .collect();
    if bullets.len() >= 2 {
        return bullets;
    }
    chunk_text(t, 400, 40)
}

/// Keyword score for retrieval without embeddings (ponytail: upgrade to real vectors later).
pub fn score_memory(content: &str, query_terms: &[String]) -> usize {
    if query_terms.is_empty() {
        return 0;
    }
    let hay = content.to_lowercase();
    query_terms
        .iter()
        .filter(|t| !t.is_empty() && hay.contains(t.as_str()))
        .count()
}

pub fn query_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let push = |t: String, terms: &mut Vec<String>| {
        if t.chars().count() >= 2 && !terms.iter().any(|x| x == &t) {
            terms.push(t);
        }
    };
    let mut latin = String::new();
    let mut cjk = String::new();
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            if !latin.is_empty() {
                push(latin.to_lowercase(), &mut terms);
                latin.clear();
            }
            cjk.push(ch);
        } else if ch.is_alphanumeric() {
            if !cjk.is_empty() {
                push(cjk.clone(), &mut terms);
                let chars: Vec<char> = cjk.chars().collect();
                if chars.len() > 2 {
                    for w in chars.windows(2) {
                        push(w.iter().collect(), &mut terms);
                    }
                }
                cjk.clear();
            }
            latin.push(ch);
        } else {
            if !latin.is_empty() {
                push(latin.to_lowercase(), &mut terms);
                latin.clear();
            }
            if !cjk.is_empty() {
                push(cjk.clone(), &mut terms);
                let chars: Vec<char> = cjk.chars().collect();
                if chars.len() > 2 {
                    for w in chars.windows(2) {
                        push(w.iter().collect(), &mut terms);
                    }
                }
                cjk.clear();
            }
        }
    }
    if !latin.is_empty() {
        push(latin.to_lowercase(), &mut terms);
    }
    if !cjk.is_empty() {
        push(cjk.clone(), &mut terms);
        let chars: Vec<char> = cjk.chars().collect();
        if chars.len() > 2 {
            for w in chars.windows(2) {
                push(w.iter().collect(), &mut terms);
            }
        }
    }
    terms.truncate(48);
    terms
}

/// Rank novel-scoped memory rows by keyword overlap; returns top `(node_id, content)`.
pub fn rank_memory(rows: &[(String, String)], query: &str, limit: usize) -> Vec<(String, String)> {
    let terms = query_terms(query);
    if terms.is_empty() || limit == 0 {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &String, &String)> = rows
        .iter()
        .map(|(nid, c)| (score_memory(c, &terms), nid, c))
        .filter(|(s, _, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, nid, c)| (nid.clone(), c.clone()))
        .collect()
}

fn search_vectors_blocking(
    novel_id: &str,
    query: &str,
    k: usize,
) -> Vec<(f64, String, String)> {
    let Ok(h) = tokio::runtime::Handle::try_current() else {
        return Vec::new();
    };
    tokio::task::block_in_place(|| h.block_on(search_vectors(novel_id, query, k))).unwrap_or_default()
}

/// MiniLM 向量优先；无命中或 Lance 空时回退关键词。`allowed` 为章节 node_id 白名单。
pub fn retrieve_memory(
    novel_id: &str,
    rows: &[(String, String)],
    query: &str,
    limit: usize,
    allowed: Option<&[String]>,
) -> Vec<(String, String)> {
    if limit == 0 || query.trim().is_empty() {
        return Vec::new();
    }
    let allow: Option<std::collections::HashSet<&str>> =
        allowed.map(|ids| ids.iter().map(String::as_str).collect());
    let hits = search_vectors_blocking(novel_id, query, (limit * 4).max(12));
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, nid, content) in hits {
        if let Some(a) = &allow {
            if !a.contains(nid.as_str()) {
                continue;
            }
        }
        if !seen.insert((nid.clone(), content.clone())) {
            continue;
        }
        out.push((nid, content));
        if out.len() >= limit {
            return out;
        }
    }
    if !out.is_empty() {
        return out;
    }
    let ranked = rank_memory(rows, query, limit);
    match &allow {
        Some(a) => ranked
            .into_iter()
            .filter(|(nid, _)| a.contains(nid.as_str()))
            .take(limit)
            .collect(),
        None => ranked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_bullets() {
        let t = "- 人物：甲｜行为：离开｜目标：寻人\n- 人物：乙｜承诺：三日后见\n杂文忽略";
        let c = split_memory_text(t);
        assert!(c.len() >= 2);
        assert!(c[0].contains("甲"));
    }

    #[test]
    fn splits_plot_and_facts() {
        let t = "【整体情节】\n甲在雾港发现旧地图，与乙约定三日后出海寻人。\n中途遭遇阻拦但脱险。\n【要点】\n- 人物：甲｜行为：发现地图｜目标：寻人\n- 人物：乙｜承诺：三日后见\n【已交代】\n- 已交代：旧地图能指向沉船";
        let c = split_memory_text(t);
        assert!(c.iter().any(|x| x.contains("整体情节") && x.contains("旧地图")), "{c:?}");
        assert!(c.iter().any(|x| x.contains("甲") && x.contains("寻人")), "{c:?}");
        assert!(c.iter().any(|x| x.contains("已交代") && x.contains("沉船")), "{c:?}");
        assert!(c.len() >= 4, "{c:?}");
    }

    #[test]
    fn scores_overlap() {
        let terms = query_terms("林潮 寻人 旧地图");
        let s = score_memory("林潮离开港口，目标是寻人并带回旧地图。", &terms);
        assert!(s >= 2, "score={s} terms={terms:?}");
    }

    #[test]
    fn retrieve_falls_back_to_keyword_without_runtime() {
        let rows = vec![(
            "n1".into(),
            "林潮离开港口，目标是寻人并带回旧地图。".into(),
        )];
        let hit = retrieve_memory("novel", &rows, "林潮 寻人", 3, None);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].0, "n1");
    }
}
