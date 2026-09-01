//! Public-library chunk embeddings via `rig-sqlite` (sqlite-vec).
//! App CRUD stays in `nove.db`; this file is only the vector index.

use crate::kb_context::MiniLmEmbedding;
use crate::paths::knowledge_vec_path;
use anyhow::{anyhow, Result};
use rig_core::embeddings::Embedding;
use rig_core::vector_store::request::{SearchFilter, VectorSearchRequestBuilder};
use rig_core::vector_store::VectorStoreIndex;
use rig_sqlite::{
    Column, ColumnValue, SqliteDistanceMetric, SqliteSearchFilter, SqliteVectorStore,
    SqliteVectorStoreTable,
};
use rusqlite::ffi::{sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
use serde::{Deserialize, Serialize};
use sqlite_vec::sqlite3_vec_init;
use std::collections::HashMap;
use std::os::raw::c_char;
use std::sync::Once;
use tokio_rusqlite::Connection;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnowledgeChunkDoc {
    pub id: String,
    pub book_id: String,
    pub idx: i64,
    pub content: String,
}

impl SqliteVectorStoreTable for KnowledgeChunkDoc {
    fn name() -> &'static str {
        "knowledge_chunks"
    }

    fn schema() -> Vec<Column> {
        vec![
            Column::new("id", "TEXT PRIMARY KEY"),
            Column::new("book_id", "TEXT").indexed(),
            Column::new("idx", "INTEGER"),
            Column::new("content", "TEXT"),
        ]
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn column_values(&self) -> Vec<(&'static str, Box<dyn ColumnValue>)> {
        vec![
            ("id", Box::new(self.id.clone())),
            ("book_id", Box::new(self.book_id.clone())),
            ("idx", Box::new(self.idx)),
            ("content", Box::new(self.content.clone())),
        ]
    }
}

type SqliteExtensionFn =
    unsafe extern "C" fn(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> i32;

fn ensure_sqlite_vec() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute::<*const (), SqliteExtensionFn>(
            sqlite3_vec_init as *const (),
        )));
    });
}

async fn vec_conn() -> Result<Connection> {
    ensure_sqlite_vec();
    let path = knowledge_vec_path();
    Connection::open(path)
        .await
        .map_err(|e| anyhow!("open knowledge_vec.db: {e}"))
}

async fn store() -> Result<SqliteVectorStore<MiniLmEmbedding, KnowledgeChunkDoc>> {
    let conn = vec_conn().await?;
    let model = MiniLmEmbedding::new();
    SqliteVectorStore::with_distance_metric(conn, &model, SqliteDistanceMetric::Cosine)
        .await
        .map_err(|e| anyhow!("init sqlite-vec store: {e}"))
}

pub async fn delete_book(book_id: &str) -> Result<()> {
    let conn = vec_conn().await?;
    let bid = book_id.to_string();
    conn.call(move |c| {
        let exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='knowledge_chunks'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Ok(());
        }
        let ids: Vec<(i64, String)> = {
            let mut stmt = c.prepare("SELECT rowid, id FROM knowledge_chunks WHERE book_id=?1")?;
            let rows = stmt.query_map([&bid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        for (rowid, _) in &ids {
            let emb_ids: Vec<i64> = {
                let mut stmt = c.prepare(
                    "SELECT embedding_rowid FROM knowledge_chunks_embedding_map WHERE document_rowid=?1",
                )?;
                let rows = stmt.query_map([rowid], |r| r.get::<_, i64>(0))?;
                rows.filter_map(|r| r.ok()).collect()
            };
            for eid in emb_ids {
                let _ = c.execute(
                    "DELETE FROM knowledge_chunks_embeddings WHERE rowid=?1",
                    [eid],
                );
            }
            let _ = c.execute(
                "DELETE FROM knowledge_chunks_embedding_map WHERE document_rowid=?1",
                [rowid],
            );
        }
        c.execute("DELETE FROM knowledge_chunks WHERE book_id=?1", [&bid])?;
        Ok(())
    })
    .await
    .map_err(|e| anyhow!("delete knowledge vec: {e}"))
}

pub async fn index_book(
    book_id: &str,
    chunks: &[(u32, String)],
    vectors: &[Vec<f32>],
) -> Result<usize> {
    if chunks.is_empty() {
        let _ = delete_book(book_id).await;
        return Ok(0);
    }
    if chunks.len() != vectors.len() {
        return Err(anyhow!("chunk/vector length mismatch"));
    }
    let _ = delete_book(book_id).await;
    let store = store().await?;
    let docs: Vec<(KnowledgeChunkDoc, Vec<Embedding>)> = chunks
        .iter()
        .zip(vectors.iter())
        .map(|((idx, content), vec)| {
            let id = format!("{book_id}#{idx}");
            (
                KnowledgeChunkDoc {
                    id: id.clone(),
                    book_id: book_id.to_string(),
                    idx: i64::from(*idx),
                    content: content.clone(),
                },
                vec![Embedding {
                    document: content.clone(),
                    vec: vec.iter().map(|x| f64::from(*x)).collect(),
                }],
            )
        })
        .collect();
    store
        .add_rows(docs)
        .await
        .map_err(|e| anyhow!("insert knowledge vec: {e}"))?;
    Ok(chunks.len())
}

pub async fn count_book(book_id: &str) -> Result<i64> {
    let conn = vec_conn().await?;
    let bid = book_id.to_string();
    conn.call(move |c| {
        let exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='knowledge_chunks'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Ok(0);
        }
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM knowledge_chunks WHERE book_id=?1",
            [&bid],
            |r| r.get(0),
        )?;
        Ok(n)
    })
    .await
    .map_err(|e| anyhow!("count knowledge vec: {e}"))
}

/// Cosine-bonus map keyed by `{book_id}#{idx}` for keyword recall.
pub async fn recall_bonus(book_ids: &[String], query: &str, k: usize) -> Result<HashMap<String, usize>> {
    if book_ids.is_empty() || query.trim().is_empty() || k == 0 {
        return Ok(HashMap::new());
    }
    let store = store().await?;
    let model = MiniLmEmbedding::new();
    let index = store.index(model);
    let filter = book_ids
        .iter()
        .map(|id| SqliteSearchFilter::eq("book_id", serde_json::Value::String(id.clone())))
        .reduce(|a, b| a.or(b));
    let mut builder = VectorSearchRequestBuilder::<SqliteSearchFilter>::default()
        .query(query)
        .samples(k as u64);
    if let Some(f) = filter {
        builder = builder.filter(f);
    }
    let req = builder.build();
    let hits = index
        .top_n::<KnowledgeChunkDoc>(req)
        .await
        .map_err(|e| anyhow!("sqlite-vec search: {e}"))?;
    let mut out = HashMap::new();
    for (score, id, _) in hits {
        if score > 0.15 {
            let bonus = (score * 20.0) as usize;
            *out.entry(id).or_default() += bonus.max(1);
        }
    }
    Ok(out)
}
