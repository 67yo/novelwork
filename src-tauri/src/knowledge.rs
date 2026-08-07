use crate::chunk::chunk_text;
use crate::paths::lancedb_dir;
use anyhow::{anyhow, Result};
use arrow_array::{ArrayRef, Int32Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema};
use epub::doc::{EpubDoc, NavPoint};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const TABLE: &str = "knowledge_chunks";

#[derive(Debug, Clone)]
pub struct SourceChapter {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSource {
    pub title: String,
    pub author: String,
    pub chapters: Vec<SourceChapter>,
}

impl ParsedSource {
    pub fn full_text(&self) -> String {
        self.chapters
            .iter()
            .map(|c| {
                if c.title.is_empty() {
                    c.text.clone()
                } else {
                    format!("【{}】\n{}", c.title, c.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn toc_lines(&self) -> String {
        self.chapters
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let t = if c.title.trim().is_empty() {
                    format!("第{}章", i + 1)
                } else {
                    c.title.clone()
                };
                format!("{}. {}", i + 1, t)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Samples for LLM: opening of first/middle/last chapters (cap total chars).
    pub fn style_samples(&self, per_chapter: usize, max_total: usize) -> String {
        if self.chapters.is_empty() {
            return String::new();
        }
        let n = self.chapters.len();
        let mut idxs = vec![0usize];
        if n > 2 {
            idxs.push(n / 2);
        }
        if n > 1 {
            idxs.push(n - 1);
        }
        idxs.sort();
        idxs.dedup();
        // also include 2nd chapter when available
        if n > 3 {
            idxs.insert(1, 1);
            idxs.sort();
            idxs.dedup();
        }
        let mut out = String::new();
        for i in idxs {
            let ch = &self.chapters[i];
            let title = if ch.title.is_empty() {
                format!("第{}章", i + 1)
            } else {
                ch.title.clone()
            };
            let sample: String = ch.text.chars().take(per_chapter).collect();
            let block = format!("—— {title} ——\n{sample}\n\n");
            if out.chars().count() + block.chars().count() > max_total {
                break;
            }
            out.push_str(&block);
        }
        out
    }
}

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

/// Best-effort remove Lance rows for a book (SQLite remains source of truth for listing).
pub async fn delete_chunks(book_id: &str) -> Result<()> {
    let db = lancedb::connect(lancedb_dir().to_str().unwrap_or("./lancedb"))
        .execute()
        .await?;
    let names = db.table_names().execute().await.unwrap_or_default();
    if !names.iter().any(|n| n == TABLE) {
        return Ok(());
    }
    let table = db.open_table(TABLE).execute().await?;
    let safe = book_id.replace('\'', "''");
    let _ = table.delete(&format!("book_id = '{safe}'")).await;
    Ok(())
}

pub fn split_book(text: &str) -> Vec<String> {
    chunk_text(text, 1000, 20)
}

/// Build storage chunks: analysis → TOC → per-chapter body.
pub fn build_knowledge_chunks(src: &ParsedSource, analysis: Option<&str>) -> Vec<String> {
    let mut chunks = Vec::new();
    if let Some(a) = analysis.map(str::trim).filter(|s| !s.is_empty()) {
        chunks.extend(split_book(&format!("【知识库分析】\n{a}")));
    }
    if src.chapters.len() > 1 {
        chunks.push(format!(
            "【目录】\n书名：{}\n作者：{}\n共 {} 章\n\n{}",
            src.title,
            src.author,
            src.chapters.len(),
            src.toc_lines()
        ));
    }
    for (i, ch) in src.chapters.iter().enumerate() {
        let head = if ch.title.trim().is_empty() {
            format!("【第{}章】", i + 1)
        } else {
            format!("【第{}章 {}】", i + 1, ch.title.trim())
        };
        chunks.extend(split_book(&format!("{head}\n{}", ch.text)));
    }
    if chunks.is_empty() {
        chunks.extend(split_book(&src.full_text()));
    }
    chunks
}

/// Guess title/author from first lines of a txt.
pub fn guess_meta(text: &str, filename: &str) -> (String, String) {
    let mut title = filename
        .trim_end_matches(".txt")
        .trim_end_matches(".TXT")
        .trim_end_matches(".epub")
        .trim_end_matches(".EPUB")
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

pub fn load_source(path: &str) -> Result<ParsedSource> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "epub" => parse_epub(path),
        "txt" => parse_txt(path),
        "pdf" => Err(anyhow!("暂不支持 PDF，请使用 txt 或 epub")),
        _ => Err(anyhow!("仅支持 txt / epub 文件")),
    }
}

fn parse_txt(path: &str) -> Result<ParsedSource> {
    let text = std::fs::read_to_string(path)?;
    let filename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名.txt");
    let (title, author) = guess_meta(&text, filename);
    Ok(ParsedSource {
        title,
        author,
        chapters: vec![SourceChapter {
            title: "全文".into(),
            text,
        }],
    })
}

fn parse_epub(path: &str) -> Result<ParsedSource> {
    let mut doc = EpubDoc::new(path).map_err(|e| anyhow!("打开 epub 失败: {e}"))?;
    let filename = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book.epub");
    let title = doc
        .get_title()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            filename
                .trim_end_matches(".epub")
                .trim_end_matches(".EPUB")
                .to_string()
        });
    let author = meta_first(&doc, &["creator", "dc:creator", "author"])
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "未知作者".into());

    let mut toc: Vec<(String, PathBuf)> = Vec::new();
    flatten_toc(&doc.toc, &mut toc);

    let mut chapters = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    if !toc.is_empty() {
        for (label, content) in toc {
            let key = strip_fragment(&content);
            let key_s = key.to_string_lossy().to_string();
            if key_s.is_empty() || !seen.insert(key_s) {
                continue;
            }
            let html = doc
                .get_resource_str_by_path(&key)
                .or_else(|| {
                    // try matching by file name among resources
                    let name = key.file_name()?.to_string_lossy().to_string();
                    let hit = doc.resources.iter().find_map(|(_, r)| {
                        r.path
                            .file_name()
                            .map(|f| f.to_string_lossy() == name)
                            .unwrap_or(false)
                            .then(|| r.path.clone())
                    })?;
                    doc.get_resource_str_by_path(hit)
                })
                .unwrap_or_default();
            let text = html_to_text(&html);
            if text.chars().count() < 40 {
                continue;
            }
            chapters.push(SourceChapter {
                title: label.trim().to_string(),
                text,
            });
        }
    }

    if chapters.is_empty() {
        let pages = doc.get_num_chapters();
        for i in 0..pages {
            if !doc.set_current_chapter(i) {
                continue;
            }
            let Some((html, mime)) = doc.get_current_str() else {
                continue;
            };
            let mime_l = mime.to_ascii_lowercase();
            if !(mime_l.contains("html") || mime_l.contains("xml")) {
                continue;
            }
            let text = html_to_text(&html);
            if text.chars().count() < 80 {
                continue;
            }
            chapters.push(SourceChapter {
                title: format!("第{}节", chapters.len() + 1),
                text,
            });
        }
    }

    if chapters.is_empty() {
        return Err(anyhow!("epub 中未解析到可用正文"));
    }

    Ok(ParsedSource {
        title,
        author,
        chapters,
    })
}

fn meta_first<R: std::io::Read + std::io::Seek>(
    doc: &EpubDoc<R>,
    keys: &[&str],
) -> Option<String> {
    for k in keys {
        if let Some(item) = doc.mdata(k) {
            let v = item.value.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn flatten_toc(nav: &[NavPoint], out: &mut Vec<(String, PathBuf)>) {
    for n in nav {
        let label = n.label.trim();
        if !label.is_empty() {
            out.push((label.to_string(), n.content.clone()));
        }
        flatten_toc(&n.children, out);
    }
}

fn strip_fragment(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    PathBuf::from(s.split('#').next().unwrap_or(""))
}

/// Minimal HTML → plain text (no extra deps).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut chars = html.chars().peekable();
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut skip_depth = 0usize; // script/style
    while let Some(c) = chars.next() {
        if c == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag = tag_buf.to_ascii_lowercase();
                let name = tag
                    .trim()
                    .trim_start_matches('/')
                    .split(|ch: char| ch.is_whitespace() || ch == '/')
                    .next()
                    .unwrap_or("");
                let closing = tag.trim_start().starts_with('/');
                if name == "script" || name == "style" {
                    if closing {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else if !tag.trim_end().ends_with('/') {
                        skip_depth += 1;
                    }
                } else if skip_depth == 0
                    && matches!(
                        name,
                        "p" | "div"
                            | "br"
                            | "li"
                            | "tr"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "section"
                            | "chapter"
                    )
                {
                    out.push('\n');
                }
                continue;
            }
            tag_buf.push(c);
            continue;
        }
        if skip_depth > 0 {
            continue;
        }
        if c == '&' {
            let mut ent = String::from("&");
            while let Some(&n) = chars.peek() {
                ent.push(n);
                chars.next();
                if n == ';' || ent.len() > 10 {
                    break;
                }
            }
            out.push_str(decode_entity(&ent));
            continue;
        }
        out.push(c);
    }
    // collapse whitespace
    let mut cleaned = String::new();
    let mut prev_space = false;
    let mut prev_nl = false;
    for c in out.chars() {
        if c == '\r' {
            continue;
        }
        if c == '\n' {
            if !prev_nl {
                cleaned.push('\n');
            }
            prev_nl = true;
            prev_space = true;
            continue;
        }
        if c.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
            }
            prev_space = true;
            prev_nl = false;
            continue;
        }
        cleaned.push(c);
        prev_space = false;
        prev_nl = false;
    }
    cleaned.trim().to_string()
}

fn decode_entity(ent: &str) -> &str {
    match ent {
        "&nbsp;" | "&#160;" => " ",
        "&amp;" => "&",
        "&lt;" => "<",
        "&gt;" => ">",
        "&quot;" => "\"",
        "&apos;" | "&#39;" => "'",
        "&mdash;" | "&#8212;" => "—",
        "&ndash;" | "&#8211;" => "–",
        "&hellip;" => "…",
        _ => " ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_entities() {
        let t = html_to_text("<p>你好&nbsp;<b>世界</b></p><script>x()</script><div>尾</div>");
        assert!(t.contains("你好"));
        assert!(t.contains("世界"));
        assert!(t.contains("尾"));
        assert!(!t.contains("script"));
        assert!(!t.contains('<'), "{t}");
    }

    #[test]
    fn build_chunks_puts_analysis_first() {
        let src = ParsedSource {
            title: "测".into(),
            author: "甲".into(),
            chapters: vec![
                SourceChapter {
                    title: "一".into(),
                    text: "甲".repeat(50),
                },
                SourceChapter {
                    title: "二".into(),
                    text: "乙".repeat(50),
                },
            ],
        };
        let chunks = build_knowledge_chunks(&src, Some("## 写作手法\n短句"));
        assert!(chunks[0].contains("知识库分析"));
        assert!(chunks.iter().any(|c| c.contains("【目录】")));
    }
}
