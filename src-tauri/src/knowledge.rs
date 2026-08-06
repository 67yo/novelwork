use crate::chunk::chunk_text;
use crate::paths::lancedb_dir;
use anyhow::Result;
use arrow_array::{ArrayRef, Int32Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};
use std::sync::Arc;

const TABLE: &str = "knowledge_chunks";

/// Persist chunks into LanceDB (text columns). Demo skips real embeddings.
pub async fn upsert_chunks(book_id: &str, chunks: &[String]) -> Result<()> {
    let db = lancedb::connect(lancedb_dir().to_str().unwrap_or("./lancedb"))
        .execute()
        .await?;

    let schema = Arc::new(Schema::new(vec![
        Field::new("book_id", DataType::Utf8, false),
        Field::new("idx", DataType::Int32, false),
        Field::new("content", DataType::Utf8, false),
    ]));

    let book_ids: StringArray = chunks.iter().map(|_| Some(book_id)).collect();
    let idxs: Int32Array = (0..chunks.len() as i32).map(Some).collect();
    let contents: StringArray = chunks.iter().map(|c| Some(c.as_str())).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(book_ids) as ArrayRef,
            Arc::new(idxs) as ArrayRef,
            Arc::new(contents) as ArrayRef,
        ],
    )?;

    let batches = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema.clone());
    let reader: Box<dyn arrow_array::RecordBatchReader + Send> = Box::new(batches);

    let names = db.table_names().execute().await.unwrap_or_default();
    if names.iter().any(|n| n == TABLE) {
        let table = db.open_table(TABLE).execute().await?;
        table.add(reader).execute().await?;
    } else {
        db.create_table(TABLE, reader).execute().await?;
    }
    Ok(())
}

pub fn split_book(text: &str) -> Vec<String> {
    chunk_text(text, 1000, 20)
}

/// Guess title/author from first lines of a txt.
pub fn guess_meta(text: &str, filename: &str) -> (String, String) {
    let mut title = filename
        .trim_end_matches(".txt")
        .trim_end_matches(".TXT")
        .to_string();
    let mut author = "未知作者".to_string();
    for line in text.lines().take(20) {
        let l = line.trim();
        if l.starts_with("书名") || l.starts_with("标题") {
            if let Some(v) = l.split(['：', ':']).nth(1) {
                title = v.trim().to_string();
            }
        }
        if l.starts_with("作者") {
            if let Some(v) = l.split(['：', ':']).nth(1) {
                author = v.trim().to_string();
            }
        }
    }
    (title, author)
}
