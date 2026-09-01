use crate::chunk::chunk_text;
use anyhow::{anyhow, Result};
use epub::doc::{EpubDoc, NavPoint};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceChapter {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSource {
    pub title: String,
    pub author: String,
    /// EPUB subject / genre metadata (may be empty for txt).
    pub subjects: Vec<String>,
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
}

pub fn split_book(text: &str) -> Vec<String> {
    chunk_text(text, 1000, 20)
}

/// Build storage chunks (no AI analysis).
/// - TXT / 单章：全文按 1000 字切段、段间重叠 20 字
/// - EPUB 多章：目录块 + 每章正文按 1000/20 切段
pub fn build_knowledge_chunks(src: &ParsedSource) -> Vec<String> {
    let mut chunks = Vec::new();
    if src.chapters.len() <= 1 {
        chunks.extend(split_book(&src.full_text()));
        return chunks;
    }
    chunks.push(format!(
        "【目录】\n书名：{}\n作者：{}\n分类：{}\n共 {} 章\n\n{}",
        src.title,
        src.author,
        if src.subjects.is_empty() {
            "—".into()
        } else {
            src.subjects.join("、")
        },
        src.chapters.len(),
        src.toc_lines()
    ));
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

/// Fetch a public URL and turn the page (or plain text) into a knowledge source.
pub async fn load_source_from_url(url: &str) -> Result<ParsedSource> {
    let (title, author, text) = fetch_url_main(url).await?;
    if text.chars().count() < 80 {
        return Err(anyhow!("网页正文过短或无法解析，请换一个公开可访问的页面"));
    }
    let text: String = text.chars().take(400_000).collect();
    Ok(ParsedSource {
        title,
        author,
        subjects: Vec::new(),
        chapters: vec![SourceChapter {
            title: "正文".into(),
            text,
        }],
    })
}

/// Fetch URL and return `(title, host/author, main_text)` — navigation chrome stripped.
pub async fn fetch_url_main(url: &str) -> Result<(String, String, String)> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow!("请填写以 http:// 或 https:// 开头的网址"));
    }
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; NovelWork/0.1; knowledge-import)")
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("网页返回错误状态: {}", resp.status()));
    }
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| anyhow!("读取网页失败: {e}"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(anyhow!("网页过大（超过 8MB），请换更精简的页面"));
    }
    let raw = String::from_utf8_lossy(&bytes);
    let looks_html = ctype.contains("html")
        || ctype.contains("xml")
        || raw.trim_start().starts_with('<');
    let host = host_from_url(url);
    let title = if looks_html {
        title_from_html(&raw).unwrap_or_else(|| host.clone())
    } else {
        host.clone()
    };
    let mut text = if looks_html {
        html_to_text(&extract_main_html(&raw))
    } else {
        raw.into_owned()
    };
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text = text.trim().to_string();
    Ok((title.trim().to_string(), host, text))
}

/// Pull `http(s)` URLs from user text (max 3).
pub fn extract_http_urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start_search = 0;
    while out.len() < 3 && start_search < text.len() {
        let slice = &text[start_search..];
        let rel = match (slice.find("https://"), slice.find("http://")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let Some(rel) = rel else { break };
        let abs = start_search + rel;
        let after = &text[abs..];
        let end_rel = after
            .char_indices()
            .find(|(_, c)| {
                c.is_whitespace() || matches!(*c, '<' | '>' | '"' | '\'' | ')' | ']' | '|' | '，' | '）' | '」' | '』')
            })
            .map(|(i, _)| i)
            .unwrap_or(after.len());
        let mut url = after[..end_rel].to_string();
        while url
            .ends_with(|c: char| matches!(c, '.' | ',' | ';' | ':' | '。' | '、' | '!'))
        {
            url.pop();
        }
        if (url.starts_with("http://") || url.starts_with("https://")) && !out.iter().any(|u| u == &url)
        {
            out.push(url);
        }
        start_search = abs + end_rel.max(1);
    }
    out
}

/// Fetch main text for URLs found in `text`; empty if none.
pub async fn fetch_urls_context(text: &str, per_url_chars: usize) -> String {
    let urls = extract_http_urls(text);
    if urls.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    for url in urls {
        match fetch_url_main(&url).await {
            Ok((title, _, body)) => {
                let body: String = body.chars().take(per_url_chars).collect();
                if body.chars().count() < 40 {
                    blocks.push(format!("【网页正文过短：{url}】"));
                } else {
                    blocks.push(format!("【网页正文：{title} | {url}】\n{body}"));
                }
            }
            Err(e) => blocks.push(format!("【网页抓取失败：{url}】\n{e}")),
        }
    }
    blocks.join("\n\n")
}

pub fn with_fetched_url_context(user_msg: &str, fetched: &str) -> String {
    if fetched.trim().is_empty() {
        user_msg.to_string()
    } else {
        format!(
            "{user_msg}\n\n----\n\
             以下为用户消息中链接的网页正文（已去除导航/页眉页脚等无关内容，请据此作答，勿臆造页面未提供的内容）：\n\
             {fetched}"
        )
    }
}

/// Prefer article/main content; drop site chrome before text conversion.
pub fn extract_main_html(html: &str) -> String {
    let try_one = |s: Option<String>| -> Option<String> {
        s.filter(|c| html_to_text(c).chars().count() >= 80)
    };
    if let Some(c) = try_one(extract_element_inner(html, "article")) {
        return c;
    }
    if let Some(c) = try_one(extract_element_inner(html, "main")) {
        return c;
    }
    if let Some(c) = try_one(extract_role_main(html)) {
        return c;
    }
    for id in [
        "content",
        "main-content",
        "article",
        "post",
        "entry",
        "mw-content-text",
    ] {
        if let Some(c) = try_one(extract_by_id(html, id)) {
            return c;
        }
    }
    for class in [
        "post-content",
        "article-content",
        "entry-content",
        "markdown-body",
    ] {
        if let Some(c) = try_one(extract_by_class_contains(html, class)) {
            return c;
        }
    }
    extract_element_inner(html, "body").unwrap_or_else(|| html.to_string())
}

fn extract_element_inner(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open_prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let bytes = lower.as_bytes();
    let mut search = 0;
    while search < bytes.len() {
        let Some(rel) = lower[search..].find(&open_prefix) else {
            return None;
        };
        let start = search + rel;
        let after_name = start + open_prefix.len();
        let boundary = bytes.get(after_name).copied().unwrap_or(b'>');
        if !(boundary == b'>' || boundary == b'/' || boundary.is_ascii_whitespace()) {
            search = after_name;
            continue;
        }
        let Some(gt_rel) = lower[start..].find('>') else {
            return None;
        };
        let gt = start + gt_rel;
        let inner_start = gt + 1;
        // self-closing
        if bytes.get(gt.saturating_sub(1)) == Some(&b'/') {
            search = inner_start;
            continue;
        }
        let mut depth = 1usize;
        let mut i = inner_start;
        while i < bytes.len() {
            if !lower.is_char_boundary(i) {
                i += 1;
                continue;
            }
            if lower[i..].starts_with(&open_prefix) {
                let b = bytes.get(i + open_prefix.len()).copied().unwrap_or(0);
                if b == b'>' || b == b'/' || b.is_ascii_whitespace() {
                    depth += 1;
                }
            }
            if lower[i..].starts_with(&close) {
                depth -= 1;
                if depth == 0 {
                    return Some(html[inner_start..i].to_string());
                }
            }
            i += 1;
        }
        return None;
    }
    None
}

fn extract_role_main(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for pat in ["role=\"main\"", "role='main'"] {
        let Some(pos) = lower.find(pat) else { continue };
        let open = html[..pos].rfind('<')?;
        let tag = tag_name_from(&lower, open)?;
        if let Some(inner) = extract_element_from(html, &lower, open, &tag) {
            return Some(inner);
        }
    }
    None
}

fn extract_by_id(html: &str, id: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let id = id.to_ascii_lowercase();
    for pat in [format!("id=\"{id}\""), format!("id='{id}'")] {
        let Some(pos) = lower.find(&pat) else { continue };
        let open = html[..pos].rfind('<')?;
        let tag = tag_name_from(&lower, open)?;
        if let Some(inner) = extract_element_from(html, &lower, open, &tag) {
            return Some(inner);
        }
    }
    None
}

fn extract_by_class_contains(html: &str, needle: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find("class=") {
        let pos = search + rel;
        let open = html[..pos].rfind('<')?;
        // class= is ASCII; scan a limited ASCII window without splitting UTF-8
        let window = lower[pos..].chars().take(80).collect::<String>();
        if !window.contains(&needle) {
            search = pos + 6;
            continue;
        }
        let tag = tag_name_from(&lower, open)?;
        if let Some(inner) = extract_element_from(html, &lower, open, &tag) {
            if html_to_text(&inner).chars().count() >= 80 {
                return Some(inner);
            }
        }
        search = pos + 6;
    }
    None
}

fn tag_name_from(lower: &str, open_lt: usize) -> Option<String> {
    let after = lower.get(open_lt + 1..)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_element_from(html: &str, lower: &str, open_lt: usize, tag: &str) -> Option<String> {
    let open_prefix = format!("<{tag}");
    let close = format!("</{tag}>");
    let bytes = lower.as_bytes();
    let Some(gt_rel) = lower[open_lt..].find('>') else {
        return None;
    };
    let gt = open_lt + gt_rel;
    let inner_start = gt + 1;
    if bytes.get(gt.saturating_sub(1)) == Some(&b'/') {
        return None;
    }
    let mut depth = 1usize;
    let mut i = inner_start;
    while i < bytes.len() {
        if !lower.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if lower[i..].starts_with(&open_prefix) {
            let b = bytes.get(i + open_prefix.len()).copied().unwrap_or(0);
            if b == b'>' || b == b'/' || b.is_ascii_whitespace() {
                depth += 1;
            }
        }
        if lower[i..].starts_with(&close) {
            depth -= 1;
            if depth == 0 {
                return Some(html[inner_start..i].to_string());
            }
        }
        i += 1;
    }
    None
}

fn title_from_html(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_open = &html[start..];
    let gt = after_open.find('>')?;
    let content = &after_open[gt + 1..];
    let end_rel = content.to_ascii_lowercase().find("</title>")?;
    let t = html_to_text(&content[..end_rel]).trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t.chars().take(120).collect())
    }
}

fn host_from_url(url: &str) -> String {
    let rest = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = rest.split('/').next().unwrap_or("网页");
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        "网页".into()
    } else {
        host.to_string()
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
        subjects: Vec::new(),
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

    let subjects = meta_all(&doc, &["subject", "dc:subject", "genre", "calibre:genre"]);

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
        subjects,
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

fn meta_all<R: std::io::Read + std::io::Seek>(doc: &EpubDoc<R>, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for item in &doc.metadata {
        let prop = item.property.to_ascii_lowercase();
        if !keys.iter().any(|k| prop == k.to_ascii_lowercase()) {
            continue;
        }
        let v = item.value.trim();
        if v.is_empty() {
            continue;
        }
        // EPUB subjects sometimes pack several with commas
        for part in v.split(|c: char| "、,，/;；|".contains(c)) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if !out.iter().any(|x: &String| x == part) {
                out.push(part.to_string());
            }
        }
    }
    out
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
                if matches!(
                    name,
                    "script"
                        | "style"
                        | "noscript"
                        | "svg"
                        | "iframe"
                        | "canvas"
                        | "nav"
                        | "header"
                        | "footer"
                        | "aside"
                        | "form"
                        | "menu"
                ) {
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
                            | "article"
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
    fn build_chunks_epub_has_toc() {
        let src = ParsedSource {
            title: "测".into(),
            author: "甲".into(),
            subjects: vec!["玄幻".into()],
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
        let chunks = build_knowledge_chunks(&src);
        assert!(chunks[0].contains("【目录】"));
        assert!(chunks[0].contains("玄幻"));
        assert!(chunks.iter().any(|c| c.contains("【第1章 一】")));
    }

    #[test]
    fn build_chunks_txt_plain_split() {
        let text: String = (0..1200).map(|_| '字').collect();
        let src = ParsedSource {
            title: "测".into(),
            author: "甲".into(),
            subjects: Vec::new(),
            chapters: vec![SourceChapter {
                title: "全文".into(),
                text: text.clone(),
            }],
        };
        let chunks = build_knowledge_chunks(&src);
        assert_eq!(chunks.len(), 2);
        assert!(!chunks[0].contains("【第"));
        assert_eq!(chunks[0].chars().count(), 1000);
    }

    #[test]
    fn host_and_title_from_html() {
        assert_eq!(host_from_url("https://wiki.example.com/foo"), "wiki.example.com");
        let t = title_from_html("<html><head><title>  雾港 · 规则  </title></head><body>x</body></html>");
        assert_eq!(t.as_deref(), Some("雾港 · 规则"));
    }

    #[test]
    fn extract_main_skips_nav() {
        let html = r#"
<html><body>
<nav><a href="/">首页</a><a href="/about">关于</a></nav>
<header>站点顶栏</header>
<article><h1>正文标题</h1><p>这是真正的正文内容，足够长以便通过阈值。</p><p>第二段继续说明设定与规则细节。</p></article>
<footer>页脚版权</footer>
</body></html>"#;
        let main = extract_main_html(html);
        let text = html_to_text(&main);
        assert!(text.contains("正文标题"), "{text}");
        assert!(text.contains("真正的正文"), "{text}");
        assert!(!text.contains("首页"), "{text}");
        assert!(!text.contains("页脚"), "{text}");
    }

    #[test]
    fn extract_http_urls_basic() {
        let u = extract_http_urls("看这个 https://example.com/a 和 http://foo.bar/x。");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0], "https://example.com/a");
        assert_eq!(u[1], "http://foo.bar/x");
    }
}
