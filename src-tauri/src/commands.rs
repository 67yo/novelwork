use crate::db::Db;
use crate::knowledge::{guess_meta, split_book, upsert_chunks};
use crate::llm;
use crate::models::*;
use crate::paths::*;
use crate::prompts::{self, PromptLocale};
use crate::AppState;
use chrono::{Local, Timelike, Utc};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{Emitter, State};
use tauri_plugin_dialog::DialogExt;

const CREATE_CHAT_CANCEL_KEY: &str = "__create__";

struct ClearChatCancel<'a> {
    state: &'a AppState,
    key: String,
}

impl Drop for ClearChatCancel<'_> {
    fn drop(&mut self) {
        self.state.clear_chat_cancel(&self.key);
    }
}

fn emit_chat_progress(app: &tauri::AppHandle, novel_id: &str, step: &str) {
    let _ = app.emit(
        "chat-progress",
        serde_json::json!({ "novelId": novel_id, "step": step }),
    );
}

/// LLM 调用并按本地小时累计 token；`novel_id` 为空则只计入全局（不归入小说报表）。
async fn llm_complete(
    state: &State<'_, AppState>,
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
    novel_id: Option<&str>,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<(String, bool), String> {
    let r = llm::complete(settings, system, user, model, cancel)
        .await
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("cancelled") {
                "cancelled".into()
            } else {
                s
            }
        })?;
    let now = Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let hour = now.hour() as u8;
    let _ = state.db.add_token_usage(
        &day,
        hour,
        &r.model,
        novel_id.unwrap_or(""),
        r.prompt_tokens,
        r.completion_tokens,
        r.total_tokens,
    );
    Ok((r.content, r.used_mock))
}

fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        if chars.is_empty() {
            String::new()
        } else {
            "*".repeat(chars.len())
        }
    } else {
        let head: String = chars.iter().take(3).collect();
        let tail: String = chars.iter().rev().take(4).rev().collect();
        format!("{head}****{tail}")
    }
}

fn apply_optional_key(target: &mut String, incoming: Option<String>) {
    let Some(k) = incoming else { return };
    let t = k.trim().to_string();
    // ignore masked paste / empty (empty = leave unchanged; keys may stay blank)
    if t.is_empty() || t.contains('*') {
        return;
    }
    *target = t;
}

/// 设置任务模型；空则回退 default_model。
fn task_model(preferred: &str, fallback: &str) -> String {
    let p = preferred.trim();
    if p.is_empty() {
        fallback.trim().to_string()
    } else {
        p.to_string()
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    let s = state.db.get_settings().map_err(|e| e.to_string())?;
    Ok(SettingsView {
        deepseek_api_key_masked: mask_key(&s.deepseek_api_key),
        deepseek_api_key_configured: !s.deepseek_api_key.trim().is_empty(),
        chatgpt_api_key_masked: mask_key(&s.chatgpt_api_key),
        chatgpt_api_key_configured: !s.chatgpt_api_key.trim().is_empty(),
        gemini_api_key_masked: mask_key(&s.gemini_api_key),
        gemini_api_key_configured: !s.gemini_api_key.trim().is_empty(),
        claude_api_key_masked: mask_key(&s.claude_api_key),
        claude_api_key_configured: !s.claude_api_key.trim().is_empty(),
        grok_api_key_masked: mask_key(&s.grok_api_key),
        grok_api_key_configured: !s.grok_api_key.trim().is_empty(),
        deepseek_base_url: s.deepseek_base_url,
        default_model: s.default_model,
        create_model: s.create_model,
        generate_model: s.generate_model,
        chat_model: s.chat_model,
        refine_model: s.refine_model,
        model_catalog: state
            .db
            .get_model_catalog()
            .unwrap_or_else(|_| ModelCatalog::seed()),
        ui_locale: s.ui_locale,
    })
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    input: SaveSettingsInput,
) -> Result<SettingsView, String> {
    let mut s = state.db.get_settings().map_err(|e| e.to_string())?;
    apply_optional_key(&mut s.deepseek_api_key, input.deepseek_api_key);
    apply_optional_key(&mut s.chatgpt_api_key, input.chatgpt_api_key);
    apply_optional_key(&mut s.gemini_api_key, input.gemini_api_key);
    apply_optional_key(&mut s.claude_api_key, input.claude_api_key);
    apply_optional_key(&mut s.grok_api_key, input.grok_api_key);
    if let Some(u) = input.deepseek_base_url {
        if !u.trim().is_empty() {
            s.deepseek_base_url = u.trim().to_string();
        }
    }
    let set_model = |slot: &mut String, v: Option<String>| {
        if let Some(m) = v {
            if !m.trim().is_empty() {
                *slot = m.trim().to_string();
            }
        }
    };
    set_model(&mut s.default_model, input.default_model);
    set_model(&mut s.create_model, input.create_model);
    set_model(&mut s.generate_model, input.generate_model);
    set_model(&mut s.chat_model, input.chat_model);
    set_model(&mut s.refine_model, input.refine_model);
    if let Some(loc) = input.ui_locale {
        let t = loc.trim();
        if !t.is_empty() {
            s.ui_locale = t.to_string();
        }
    }
    state.db.save_settings(&s).map_err(|e| e.to_string())?;
    get_settings(state)
}

#[tauri::command]
pub async fn refresh_model_catalog(state: State<'_, AppState>) -> Result<ModelCatalog, String> {
    crate::catalog::refresh(&state.db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_genres() -> Vec<String> {
    GENRES.iter().map(|s| (*s).to_string()).collect()
}

#[tauri::command]
pub fn list_knowledge_bases(state: State<'_, AppState>) -> Result<Vec<KnowledgeBook>, String> {
    state.db.list_knowledge().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_knowledge_text(
    state: State<'_, AppState>,
    path: String,
    extract_prompt: String,
    genres: Vec<String>,
) -> Result<KnowledgeBook, String> {
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| e.to_string())?;
    let filename = Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("未命名.txt")
        .to_string();
    let (mut title, mut author) = guess_meta(&text, &filename);

    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let excerpt: String = text.chars().take(2000).collect();
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &[&extract_prompt, &excerpt]);
    if !settings.deepseek_api_key.trim().is_empty() && !extract_prompt.trim().is_empty() {
        let sys = prompts::knowledge_extract_system(loc);
        let user = prompts::knowledge_extract_user(loc, &extract_prompt, &excerpt);
        if let Ok((out, _)) = llm_complete(&state, &settings, sys, &user, None, None, None).await {
            for line in out.lines() {
                let (t, a) = prompts::parse_title_author(loc, line);
                if let Some(v) = t {
                    title = v;
                }
                if let Some(v) = a {
                    author = v;
                }
            }
        }
    }

    let chunks = split_book(&text);
    let book = KnowledgeBook {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        author,
        genres,
        source_path: path,
        extract_prompt,
        created_at: Utc::now().to_rfc3339(),
        chunk_count: chunks.len() as i64,
    };
    state
        .db
        .insert_knowledge(&book, &chunks)
        .map_err(|e| e.to_string())?;
    // LanceDB best-effort
    let _ = upsert_chunks(&book.id, &chunks).await;
    Ok(book)
}

#[tauri::command]
pub fn list_novels(state: State<'_, AppState>) -> Result<Vec<NovelProject>, String> {
    state.db.list_novels().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_novel(state: State<'_, AppState>, id: String) -> Result<Option<NovelProject>, String> {
    state.db.get_novel(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_novel(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    novel.archived = archived;
    novel.updated_at = Utc::now().to_rfc3339();
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    let _ = fs::write(
        novel_meta_path(&id),
        serde_json::to_string_pretty(&novel).unwrap_or_default(),
    );
    Ok(novel)
}

#[tauri::command]
pub fn delete_novel(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let novel = state
        .db
        .get_novel(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    if !novel.archived {
        return Err("请先归档后再删除".into());
    }
    state.db.delete_novel(&id).map_err(|e| e.to_string())?;
    let dir = novel_dir(&id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn create_novel(
    state: State<'_, AppState>,
    input: CreateNovelInput,
) -> Result<NovelProject, String> {
    create_novel_with_tree(
        &state,
        &input.title,
        &input.synopsis,
        &input.knowledge_ids,
        &input.knowledge_strategy,
        2000,
        3000,
        20,
        None,
        None,
    )
}

/// 统计正文有效字数（去掉空白）。
fn count_words(text: &str) -> u32 {
    text.chars().filter(|c| !c.is_whitespace()).count() as u32
}

fn normalize_word_range(min: u32, max: u32) -> (u32, u32) {
    if min == 0 && max == 0 {
        (2000, 3000)
    } else if max < min {
        (max.max(1), min)
    } else {
        (min.max(1), max.max(min.max(1)))
    }
}

/// 从用户话里抠每章目标字数（不依赖模型是否吐 JSON）。
fn parse_word_target_from_text(text: &str) -> Option<(u32, u32)> {
    let lower = text.to_lowercase();
    // 收集 3–5 位数字
    let mut nums: Vec<u32> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if (3..=5).contains(&cur.len()) {
                if let Ok(n) = cur.parse::<u32>() {
                    if (500..=20000).contains(&n) {
                        nums.push(n);
                    }
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && (3..=5).contains(&cur.len()) {
        if let Ok(n) = cur.parse::<u32>() {
            if (500..=20000).contains(&n) {
                nums.push(n);
            }
        }
    }
    if nums.is_empty() {
        return None;
    }

    let mentions_chapter_words = text.contains('字')
        || lower.contains("word")
        || lower.contains("字数")
        || text.contains("每章")
        || lower.contains("per chapter")
        || lower.contains("chapter length")
        || lower.contains("target");
    if !mentions_chapter_words {
        return None;
    }

    // 区间：出现两个数字且文中有 –/-/到/至/~
    if nums.len() >= 2
        && (text.contains('–')
            || text.contains('-')
            || text.contains('—')
            || text.contains('~')
            || text.contains('到')
            || text.contains('至')
            || lower.contains(" to "))
    {
        return Some(normalize_word_range(nums[0], nums[1]));
    }
    let n = *nums.last()?;
    Some((n, n))
}

fn word_target_from_json(v: &serde_json::Value) -> Option<(u32, u32)> {
    if let Some(wc) = v.get("word_count") {
        if let Some(n) = wc.as_u64() {
            let n = n as u32;
            return Some(normalize_word_range(n, n));
        }
        let min = wc
            .get("min")
            .or_else(|| wc.get("word_count_min"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let max = wc
            .get("max")
            .or_else(|| wc.get("word_count_max"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32);
        let target = wc.get("target").and_then(|x| x.as_u64()).map(|x| x as u32);
        if let (Some(a), Some(b)) = (min, max) {
            return Some(normalize_word_range(a, b));
        }
        if let Some(t) = target.or(min).or(max) {
            return Some(normalize_word_range(t, t));
        }
    }
    let min = v
        .get("word_count_min")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    let max = v
        .get("word_count_max")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    match (min, max) {
        (Some(a), Some(b)) => Some(normalize_word_range(a, b)),
        (Some(a), None) => Some(normalize_word_range(a, a)),
        (None, Some(b)) => Some(normalize_word_range(b, b)),
        _ => None,
    }
}

fn apply_novel_word_target(
    state: &State<'_, AppState>,
    novel: &mut NovelProject,
    tree: &mut NovelTree,
    min: u32,
    max: u32,
) -> Result<(), String> {
    let (wmin, wmax) = normalize_word_range(min, max);
    novel.word_count_min = wmin;
    novel.word_count_max = wmax;
    novel.updated_at = Utc::now().to_rfc3339();
    if let Some(root) = tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        root.word_count_min = wmin;
        root.word_count_max = wmax;
    }
    state.db.upsert_novel(novel).map_err(|e| e.to_string())
}

fn normalize_chapter_count(n: u32) -> u32 {
    n.clamp(1, 500)
}

fn chapter_count_from_json(v: &serde_json::Value) -> Option<u32> {
    if let Some(n) = v.get("chapter_count").and_then(|x| x.as_u64()) {
        return Some(normalize_chapter_count(n as u32));
    }
    if let Some(c) = v.get("chapters") {
        if let Some(n) = c.as_u64() {
            return Some(normalize_chapter_count(n as u32));
        }
        if let Some(n) = c
            .get("total")
            .or_else(|| c.get("count"))
            .and_then(|x| x.as_u64())
        {
            return Some(normalize_chapter_count(n as u32));
        }
    }
    None
}

/// 从用户话里抠全书计划章数（如「一共20章」「计划 30 章」「20 chapters」）。
fn parse_chapter_count_from_text(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let hint = text.contains("一共")
        || text.contains("总共")
        || text.contains("全书")
        || text.contains("整本")
        || (text.contains("计划") && text.contains("章"))
        || (text.contains("预计") && text.contains("章"))
        || lower.contains("total chapters")
        || lower.contains("chapter count")
        || lower.contains("chapters total")
        || (lower.contains("chapters")
            && (lower.contains("total") || lower.contains("about") || lower.contains("plan")));
    if !hint {
        return None;
    }
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<u32>() {
                if (1..=500).contains(&n) {
                    nums.push(n);
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(n) = cur.parse::<u32>() {
            if (1..=500).contains(&n) {
                nums.push(n);
            }
        }
    }
    // 优先取 ≤200 的数（避开误抓字数）；否则取第一个合法值
    nums.iter()
        .copied()
        .find(|n| (1..=200).contains(n))
        .or_else(|| nums.first().copied())
        .map(normalize_chapter_count)
}

fn apply_novel_chapter_count(
    state: &State<'_, AppState>,
    novel: &mut NovelProject,
    tree: &mut NovelTree,
    count: u32,
) -> Result<(), String> {
    let n = normalize_chapter_count(count);
    novel.chapter_count = n;
    novel.updated_at = Utc::now().to_rfc3339();
    if let Some(root) = tree
        .nodes
        .iter_mut()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        root.chapter_count = n;
    }
    state.db.upsert_novel(novel).map_err(|e| e.to_string())
}

fn set_node_word_count(tree: &mut NovelTree, node_id: &str, words: u32) {
    if let Some(n) = tree.nodes.iter_mut().find(|n| n.id == node_id) {
        n.word_count = words;
    }
}

fn create_novel_with_tree(
    state: &State<'_, AppState>,
    title: &str,
    synopsis: &str,
    knowledge_ids: &[String],
    knowledge_strategy: &str,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
    _chapters: Option<&[serde_json::Value]>,
    characters: Option<&[serde_json::Value]>,
) -> Result<NovelProject, String> {
    let now = Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let (wmin, wmax) = normalize_word_range(word_count_min, word_count_max);
    let chapters_n = normalize_chapter_count(if chapter_count == 0 {
        20
    } else {
        chapter_count
    });
    let novel = NovelProject {
        id: id.clone(),
        title: title.to_string(),
        synopsis: synopsis.to_string(),
        cover_path: None,
        knowledge_ids: knowledge_ids.to_vec(),
        knowledge_strategy: if knowledge_strategy.is_empty() {
            "参考对话中确定的叙事与人物约束。".into()
        } else {
            knowledge_strategy.to_string()
        },
        archived: false,
        word_count_min: wmin,
        word_count_max: wmax,
        chapter_count: chapters_n,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    fs::write(
        novel_meta_path(&id),
        serde_json::to_string_pretty(&novel).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let tree = build_initial_tree(&id, title, synopsis, wmin, wmax, chapters_n, characters);
    fs::write(
        novel_tree_path(&id),
        serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(novel)
}

fn build_initial_tree(
    novel_id: &str,
    title: &str,
    synopsis: &str,
    word_count_min: u32,
    word_count_max: u32,
    chapter_count: u32,
    characters: Option<&[serde_json::Value]>,
) -> NovelTree {
    let root = "root".to_string();
    let mut nodes = vec![TreeNode {
        id: root.clone(),
        kind: NodeKind::Novel,
        label: title.to_string(),
        outline: synopsis.to_string(),
        character: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        position: NodePosition { x: 280.0, y: 0.0 },
        word_count: 0,
        word_count_min,
        word_count_max,
        chapter_count,
    }];
    let mut edges = vec![];

    // ponytail: 开树不建章节；用户定内容后经 Chat 生成大纲再挂章节节点
    if let Some(chars) = characters {
        for (i, c) in chars.iter().enumerate() {
            let cid = format!("char-{}", i + 1);
            let label = c
                .get("label")
                .and_then(|x| x.as_str())
                .unwrap_or("新角色")
                .to_string();
            let y = 80.0 + 140.0 * (i as f64);
            nodes.push(TreeNode {
                id: cid.clone(),
                kind: NodeKind::Character,
                label,
                outline: String::new(),
                character: Some(CharacterCard {
                    role: c
                        .get("role")
                        .and_then(|x| x.as_str())
                        .unwrap_or("配角")
                        .into(),
                    personality: c
                        .get("personality")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                    gender: c
                        .get("gender")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                    alignment: c
                        .get("alignment")
                        .and_then(|x| x.as_str())
                        .unwrap_or("中立")
                        .into(),
                }),
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                position: NodePosition {
                    x: 40.0,
                    y: y + 20.0,
                },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            });
            if let Some(root_node) = nodes.iter_mut().find(|n| n.id == root) {
                root_node.linked_character_ids.push(cid.clone());
            }
            edges.push(TreeEdge {
                id: format!("e-{root}-{cid}"),
                source: root.clone(),
                target: cid,
                kind: "character".into(),
                source_handle: Some("left".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            });
        }
    }

    NovelTree {
        novel_id: novel_id.into(),
        nodes,
        edges,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChapterResolveMode {
    /// 更新已有同号章节的标题/大纲，缺失则追加
    Overwrite,
    /// 只追加树上还没有的编号
    Skip,
    /// 一律追加新节点（允许重复章号）
    ForceAppend,
}

fn root_id(tree: &NovelTree) -> String {
    tree.nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
        .unwrap_or_else(|| "root".into())
}

fn cn_digit(ch: char) -> Option<u32> {
    match ch {
        '零' | '〇' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '两' | '兩' | '贰' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' | '陸' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn is_cn_num_char(ch: char) -> bool {
    cn_digit(ch).is_some() || ch == '十' || ch == '百'
}

/// 中文数字 → u32（常见章号：一…九十九、一百…）。
fn parse_chinese_numeral(s: &str) -> Option<u32> {
    let chars: Vec<char> = s.chars().filter(|c| is_cn_num_char(*c)).collect();
    if chars.is_empty() {
        return None;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '百' {
            if n == 0 {
                n = 1;
            }
            n = n.saturating_mul(100);
            i += 1;
            continue;
        }
        if c == '十' {
            if n == 0 {
                n = 1;
            }
            n = n.saturating_mul(10);
            i += 1;
            continue;
        }
        let Some(v) = cn_digit(c) else {
            return None;
        };
        if v == 0 {
            i += 1;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == '百' {
            n = n.saturating_add(v.saturating_mul(100));
            i += 2;
            continue;
        }
        if i + 1 < chars.len() && chars[i + 1] == '十' {
            n = n.saturating_add(v.saturating_mul(10));
            i += 2;
            continue;
        }
        n = n.saturating_add(v);
        i += 1;
    }
    if (1..=500).contains(&n) {
        Some(n)
    } else {
        None
    }
}

fn parse_chapter_num_token(tok: &str) -> Option<u32> {
    let tok = tok.trim();
    if tok.is_empty() {
        return None;
    }
    if let Ok(n) = tok.parse::<u32>() {
        return (1..=500).contains(&n).then_some(n);
    }
    parse_chinese_numeral(tok)
}

fn chapter_number_from_label(label: &str) -> Option<u32> {
    if let Some(i) = label.find('第') {
        let rest = &label[i + '第'.len_utf8()..];
        let mut body = String::new();
        let mut rem = rest;
        for (idx, ch) in rest.char_indices() {
            if ch == '章' {
                return parse_chapter_num_token(&body);
            }
            if ch.is_whitespace() {
                continue;
            }
            if ch.is_ascii_digit() || is_cn_num_char(ch) {
                body.push(ch);
                rem = &rest[idx + ch.len_utf8()..];
            } else {
                rem = &rest[idx..];
                break;
            }
        }
        // 允许「第1」「第一」省略「章」；若后面是区间词则不是单章号
        let rem = rem.trim_start();
        if !body.is_empty()
            && (rem.is_empty()
                || rem.starts_with('·')
                || rem.starts_with('・')
                || rem.starts_with('—'))
        {
            return parse_chapter_num_token(&body);
        }
    }
    // 「10章」「十二章」无「第」
    if let Some(j) = label.find('章') {
        let head = label[..j].trim();
        // 避免把「第一到第十章」整段误解析
        if !head.contains('到') && !head.contains('至') && !head.contains('-') {
            if let Some(n) = parse_chapter_num_token(head) {
                return Some(n);
            }
        }
    }
    let lower = label.to_lowercase();
    if let Some(i) = lower.find("chapter") {
        let rest = lower[i + "chapter".len()..].trim_start();
        let mut body = String::new();
        for ch in rest.chars() {
            if ch.is_ascii_digit() {
                body.push(ch);
            } else if body.is_empty() && (ch.is_whitespace() || ch == '.' || ch == '#') {
                continue;
            } else {
                break;
            }
        }
        return parse_chapter_num_token(&body);
    }
    None
}

/// 从指令参数里解析章号：`3` / `第三章` / `第三章 · 夜` / `一`。
fn parse_chapter_ref(args: &str) -> Option<u32> {
    let a = args.trim();
    if a.is_empty() {
        return None;
    }
    if let Some(n) = chapter_number_from_label(a) {
        return Some(n);
    }
    // 取第一个空白前的 token
    let token = a.split_whitespace().next().unwrap_or(a);
    if let Some(n) = chapter_number_from_label(token) {
        return Some(n);
    }
    if token.ends_with('章') {
        if let Some(n) = chapter_number_from_label(&format!("第{token}")) {
            return Some(n);
        }
    } else if let Some(n) = chapter_number_from_label(&format!("第{token}章")) {
        return Some(n);
    }
    // token 自身可能是「一」「十二」
    parse_chapter_num_token(token).or_else(|| extract_small_chapter_nums(a).into_iter().next())
}

/// 章号 → 节点 id（同号多个时取 y 最小的一个）
fn existing_chapter_nums(tree: &NovelTree) -> std::collections::HashMap<u32, String> {
    let mut map = std::collections::HashMap::new();
    let mut chapters: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for n in chapters {
        if let Some(num) = chapter_number_from_label(&n.label) {
            map.entry(num).or_insert_with(|| n.id.clone());
        }
    }
    map
}

fn last_chapter_anchor(tree: &NovelTree) -> (String, f64, f64) {
    let root = root_id(tree);
    let mut chapters: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(last) = chapters.last() {
        (last.id.clone(), last.position.x, last.position.y)
    } else if let Some(r) = tree.nodes.iter().find(|n| n.id == root) {
        (root, r.position.x, r.position.y)
    } else {
        (root, 280.0, 0.0)
    }
}

/// 删除全部章节节点及其相关边（角色/剧情卡保留）。
fn clear_all_chapter_nodes(tree: &mut NovelTree) {
    let old: Vec<String> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .map(|n| n.id.clone())
        .collect();
    if old.is_empty() {
        return;
    }
    tree.nodes.retain(|n| !matches!(n.kind, NodeKind::Chapter));
    tree.edges.retain(|e| {
        e.kind != "chapter" && !old.iter().any(|id| e.source == *id || e.target == *id)
    });
    let alive: std::collections::HashSet<String> =
        tree.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut tree.nodes {
        n.linked_character_ids.retain(|id| alive.contains(id));
        n.linked_side_plot_ids.retain(|id| alive.contains(id));
    }
}

fn push_chapter_node(
    tree: &mut NovelTree,
    prev: &mut String,
    x: f64,
    y: &mut f64,
    label: String,
    outline: String,
) {
    *y += 140.0;
    let cid = uuid::Uuid::new_v4().to_string();
    tree.nodes.push(TreeNode {
        id: cid.clone(),
        kind: NodeKind::Chapter,
        label,
        outline,
        character: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        position: NodePosition { x, y: *y },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    tree.edges.push(TreeEdge {
        id: format!("e-{prev}-{cid}"),
        source: prev.clone(),
        target: cid.clone(),
        kind: "chapter".into(),
        source_handle: Some("bottom".into()),
        target_handle: Some("top".into()),
        label: String::new(),
    });
    *prev = cid;
}

/// 将大纲挂到树上：按模式追加/覆盖，不再默认整链替换。
fn apply_chapter_outlines(
    tree: &mut NovelTree,
    list: &[serde_json::Value],
    mode: ChapterResolveMode,
) {
    let existing = existing_chapter_nums(tree);
    let (mut prev, x, mut y) = last_chapter_anchor(tree);

    for (i, c) in list.iter().enumerate() {
        let n = c
            .get("n")
            .or_else(|| c.get("index"))
            .and_then(|x| x.as_u64())
            .map(|x| x as u32)
            .or_else(|| {
                c.get("label")
                    .or_else(|| c.get("title"))
                    .and_then(|x| x.as_str())
                    .and_then(chapter_number_from_label)
            })
            .unwrap_or((i + 1) as u32);
        let label = c
            .get("label")
            .or_else(|| c.get("title"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("第{n}章"));
        let outline = c
            .get("outline")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        match mode {
            ChapterResolveMode::Skip if existing.contains_key(&n) => continue,
            ChapterResolveMode::Overwrite => {
                if let Some(id) = existing.get(&n) {
                    if let Some(node) = tree.nodes.iter_mut().find(|x| x.id == *id) {
                        node.label = label;
                        node.outline = outline;
                    }
                    continue;
                }
                push_chapter_node(tree, &mut prev, x, &mut y, label, outline);
            }
            ChapterResolveMode::Skip | ChapterResolveMode::ForceAppend => {
                push_chapter_node(tree, &mut prev, x, &mut y, label, outline);
            }
        }
    }
}

fn add_plot_to_chapter(
    tree: &mut NovelTree,
    chapter_num: u32,
    plot_text: &str,
) -> Result<String, String> {
    let existing = existing_chapter_nums(tree);
    let chapter_id = existing
        .get(&chapter_num)
        .cloned()
        .ok_or_else(|| format!("结构树上找不到第{chapter_num}章，请先生成章节卡。"))?;
    let plot_text = plot_text.trim();
    if plot_text.is_empty() {
        return Err("剧情内容为空。".into());
    }
    let label: String = plot_text.chars().take(24).collect();
    let label = if plot_text.chars().count() > 24 {
        format!("{label}…")
    } else {
        label
    };
    let plots = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::SidePlot))
        .count();
    let chapter_pos = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .map(|n| n.position.clone())
        .unwrap_or(NodePosition { x: 280.0, y: 140.0 });
    let pid = uuid::Uuid::new_v4().to_string();
    tree.nodes.push(TreeNode {
        id: pid.clone(),
        kind: NodeKind::SidePlot,
        label,
        outline: plot_text.to_string(),
        character: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        position: NodePosition {
            x: chapter_pos.x + 280.0,
            y: chapter_pos.y + plots as f64 * 24.0,
        },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    });
    if let Some(ch) = tree.nodes.iter_mut().find(|n| n.id == chapter_id) {
        ch.linked_side_plot_ids.push(pid.clone());
    }
    tree.edges.push(TreeEdge {
        id: format!("e-{chapter_id}-{pid}"),
        source: chapter_id,
        target: pid,
        kind: "side_plot".into(),
        source_handle: Some("right".into()),
        target_handle: Some("left".into()),
        label: String::new(),
    });
    Ok(format!(
        "已为第{chapter_num}章添加剧情卡，可继续追加更多剧情。"
    ))
}

fn parse_resolve_mode(text: &str) -> Option<ChapterResolveMode> {
    let lower = text.to_lowercase();
    if text.contains("覆盖") || text.contains("替换") || lower.contains("overwrite") {
        Some(ChapterResolveMode::Overwrite)
    } else if text.contains("跳过") || text.contains("只补") || lower.contains("skip") {
        Some(ChapterResolveMode::Skip)
    } else if text.contains("强制") || text.contains("仍要追加") || lower.contains("force") {
        Some(ChapterResolveMode::ForceAppend)
    } else {
        None
    }
}

fn extract_small_chapter_nums(text: &str) -> Vec<u32> {
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if (1..=3).contains(&cur.len()) {
                if let Ok(n) = cur.parse::<u32>() {
                    if (1..=500).contains(&n) {
                        nums.push(n);
                    }
                }
            }
            cur.clear();
        }
    }
    if !cur.is_empty() && (1..=3).contains(&cur.len()) {
        if let Ok(n) = cur.parse::<u32>() {
            if (1..=500).contains(&n) {
                nums.push(n);
            }
        }
    }
    nums
}

/// `/指令名 args` → 返回 args；按名称长度优先匹配。
fn slash_args<'a>(text: &'a str, names: &[&str]) -> Option<&'a str> {
    let t = text.trim();
    let rest = t.strip_prefix('/')?;
    let mut names: Vec<&str> = names.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        if let Some(after) = rest.strip_prefix(name) {
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                return Some(after.trim());
            }
        }
    }
    None
}

fn root_slash_help(zh: bool) -> String {
    if zh {
        "根节点指令（以 / 开头）：\n\
         · `/章节卡 1-10` / `/章节卡 第 1-10 章` / `/章节卡 第一到第十章` — 生成章节卡并追加\n\
         · `/章节卡 覆盖 1-10` / `/章节卡 跳过 1-10` / `/章节卡 强制追加 1-10` — 冲突时的处理\n\
         · `/添加剧情 3 剧情内容`（也支持 `/添加剧情 第一章 …`）\n\
         · `/剧情卡 3` 或 `/剧情卡 第一章` — 按该章大纲拆多张剧情卡并补人物\n\
         · `/清空章节` — 删除全部章节节点"
            .into()
    } else {
        "Root slash commands:\n\
         · `/chapters 1-10` — generate & append chapter cards\n\
         · `/chapters overwrite|skip|force 1-10` — conflict mode\n\
         · `/add-plot 3 plot text` — add one plot card to chapter 3\n\
         · `/plots 3` — auto plot cards from chapter 3 outline + characters\n\
         · `/clear-chapters` — delete all chapter nodes"
            .into()
    }
}

fn parse_clear_all_chapters(text: &str) -> bool {
    slash_args(
        text,
        &[
            "清空章节",
            "清空所有章节",
            "清空所有章节节点",
            "clear-chapters",
            "clear_chapters",
        ],
    )
    .is_some()
}

/// `/剧情卡 3` · `/剧情卡 第一章`
fn parse_gen_plots_for_chapter(text: &str) -> Option<u32> {
    let args = slash_args(text, &["生成剧情卡", "剧情卡", "plots", "gen-plots"])?;
    parse_chapter_ref(args)
}

/// 章节卡 Chat：`/完善剧情`（当前章，无需编号）
fn parse_enrich_plots_command(text: &str) -> bool {
    slash_args(
        text,
        &["完善剧情", "生成剧情", "enrich-plots", "enrich_plots"],
    )
    .is_some()
}

/// 按章节大纲拆剧情卡并写入树；返回带落库说明的回复。
async fn enrich_plots_for_chapter_node(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    settings: &AppSettings,
    novel: &NovelProject,
    novel_id: &str,
    tree: &mut NovelTree,
    chapter_id: &str,
    chapter_num: Option<u32>,
    loc: PromptLocale,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<String, String> {
    let chapter = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .cloned()
        .ok_or_else(|| "章节节点丢失".to_string())?;
    if chapter.outline.trim().is_empty() {
        return Ok(if loc.is_zh() {
            format!(
                "本章「{}」尚无大纲，请先完善章节大纲再执行 /完善剧情。",
                chapter.label
            )
        } else {
            format!(
                "Chapter “{}” has no outline yet. Fill the outline first.",
                chapter.label
            )
        });
    }
    let existing_chars: String = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| {
            let card = n.character.as_ref();
            format!(
                "- {} | {} | {}",
                n.label,
                card.map(|c| c.role.as_str()).unwrap_or(""),
                card.map(|c| c.personality.as_str()).unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    emit_chat_progress(app, novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (_def_label, def_role, def_align) = prompts::default_new_character(loc);
    let num = chapter_num.unwrap_or_else(|| chapter_number_from_label(&chapter.label).unwrap_or(0));
    let system =
        prompts::gen_chapter_plots_system(loc, &novel.title, &novel.synopsis, novel.chapter_count);
    let user = prompts::gen_chapter_plots_user(
        loc,
        num,
        &chapter.label,
        &chapter.outline,
        &existing_chars,
    );
    let (reply, _) = llm_complete(
        state,
        settings,
        &system,
        &user,
        Some(&chat_model),
        Some(novel_id),
        cancel,
    )
    .await?;
    let mut added_plots = 0usize;
    let mut added_chars = 0usize;
    for line in reply.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(list) = v.get("plots").and_then(|x| x.as_array()) {
            if !list.is_empty() {
                emit_chat_progress(app, novel_id, "apply_card");
                let (p, c) = apply_plots_and_characters_for_chapter(
                    tree, chapter_id, list, def_role, def_align,
                );
                added_plots = p;
                added_chars = c;
                break;
            }
        }
    }
    if added_plots > 0 {
        let _ = save_tree(tree.clone());
    }
    emit_chat_progress(app, novel_id, "saving");
    let note = if loc.is_zh() {
        if added_plots > 0 {
            format!(
                "\n\n（已为本章新增 {added_plots} 张剧情卡，补齐人物卡 {added_chars} 张，并已关联。）"
            )
        } else {
            "\n\n（未解析到 plots JSON，结构树未改动。请重试。）".into()
        }
    } else if added_plots > 0 {
        format!("\n\n(Added {added_plots} plot card(s) and {added_chars} new character(s).)")
    } else {
        "\n\n(No plots JSON found; tree unchanged.)".into()
    };
    Ok(format!("{reply}{note}"))
}

fn find_character_id_by_label(tree: &NovelTree, label: &str) -> Option<String> {
    let key = label.trim();
    if key.is_empty() {
        return None;
    }
    tree.nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .find(|n| {
            let l = n.label.trim();
            l == key || l.contains(key) || key.contains(l)
        })
        .map(|n| n.id.clone())
}

fn ensure_character_linked(tree: &mut NovelTree, host_id: &str, char_id: &str) {
    if let Some(host) = tree.nodes.iter_mut().find(|n| n.id == host_id) {
        if !host.linked_character_ids.contains(&char_id.to_string()) {
            host.linked_character_ids.push(char_id.to_string());
        }
    }
    let exists = tree.edges.iter().any(|e| {
        e.kind == "character"
            && ((e.source == host_id && e.target == char_id)
                || (e.source == char_id && e.target == host_id))
    });
    if !exists {
        tree.edges.push(TreeEdge {
            id: format!("e-{host_id}-{char_id}"),
            source: host_id.to_string(),
            target: char_id.to_string(),
            kind: "character".into(),
            source_handle: Some("left".into()),
            target_handle: Some("right".into()),
            label: String::new(),
        });
    }
}

/// 根据 JSON 为某章追加多张剧情卡，并补齐/关联人物卡。
fn apply_plots_and_characters_for_chapter(
    tree: &mut NovelTree,
    chapter_id: &str,
    plots: &[serde_json::Value],
    def_role: &str,
    def_align: &str,
) -> (usize, usize) {
    let chapter_pos = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .map(|n| n.position.clone())
        .unwrap_or(NodePosition { x: 280.0, y: 140.0 });
    let mut plot_count = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::SidePlot))
        .count();
    let mut char_y = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| n.position.y)
        .fold(80.0_f64, f64::max);
    let mut added_plots = 0usize;
    let mut added_chars = 0usize;

    for p in plots {
        let label = p
            .get("label")
            .or_else(|| p.get("title"))
            .and_then(|x| x.as_str())
            .unwrap_or("剧情")
            .to_string();
        let outline = p
            .get("outline")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if label.trim().is_empty() && outline.trim().is_empty() {
            continue;
        }
        let pid = uuid::Uuid::new_v4().to_string();
        tree.nodes.push(TreeNode {
            id: pid.clone(),
            kind: NodeKind::SidePlot,
            label: if label.trim().is_empty() {
                outline.chars().take(24).collect()
            } else {
                label
            },
            outline,
            character: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            position: NodePosition {
                x: chapter_pos.x + 280.0,
                y: chapter_pos.y + plot_count as f64 * 36.0,
            },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        });
        plot_count += 1;
        if let Some(ch) = tree.nodes.iter_mut().find(|n| n.id == chapter_id) {
            ch.linked_side_plot_ids.push(pid.clone());
        }
        tree.edges.push(TreeEdge {
            id: format!("e-{chapter_id}-{pid}"),
            source: chapter_id.to_string(),
            target: pid.clone(),
            kind: "side_plot".into(),
            source_handle: Some("right".into()),
            target_handle: Some("left".into()),
            label: String::new(),
        });
        added_plots += 1;

        let chars = p
            .get("characters")
            .or_else(|| p.get("roles"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for c in chars {
            let clabel = c
                .get("label")
                .or_else(|| c.get("name"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if clabel.is_empty() {
                continue;
            }
            let cid = if let Some(id) = find_character_id_by_label(tree, &clabel) {
                id
            } else {
                char_y += 120.0;
                let cid = uuid::Uuid::new_v4().to_string();
                tree.nodes.push(TreeNode {
                    id: cid.clone(),
                    kind: NodeKind::Character,
                    label: clabel,
                    outline: String::new(),
                    character: Some(CharacterCard {
                        role: c
                            .get("role")
                            .and_then(|x| x.as_str())
                            .unwrap_or(def_role)
                            .into(),
                        personality: c
                            .get("personality")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                        gender: c
                            .get("gender")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .into(),
                        style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                        alignment: c
                            .get("alignment")
                            .and_then(|x| x.as_str())
                            .unwrap_or(def_align)
                            .into(),
                    }),
                    linked_character_ids: vec![],
                    linked_side_plot_ids: vec![],
                    position: NodePosition { x: 40.0, y: char_y },
                    word_count: 0,
                    word_count_min: 0,
                    word_count_max: 0,
                    chapter_count: 0,
                });
                added_chars += 1;
                cid
            };
            ensure_character_linked(tree, &pid, &cid);
            ensure_character_linked(tree, chapter_id, &cid);
        }
    }
    (added_plots, added_chars)
}

fn strip_resolve_prefix(args: &str) -> &str {
    let t = args.trim();
    for p in [
        "覆盖冲突",
        "覆盖已有",
        "覆盖",
        "跳过已有",
        "跳过冲突",
        "跳过",
        "强制追加",
        "强制",
        "只补缺",
    ] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim();
        }
    }
    let lower = t.to_lowercase();
    for p in ["overwrite", "skip", "force"] {
        if lower.starts_with(p) {
            return t[p.len()..].trim();
        }
    }
    t
}

/// 解析章号区间：第一到第十章 / 第1-10章 / 第 1-10 章 / 1-10 / 第一章-第十章
fn parse_chapter_range(args: &str) -> Option<(u32, u32)> {
    let raw = strip_resolve_prefix(args);
    // 去掉空白，便于匹配「第 1-10 章」
    let s: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return None;
    }

    let pair = |a: &str, b: &str| -> Option<(u32, u32)> {
        let x = parse_chapter_ref(a)?;
        let y = parse_chapter_ref(b)?;
        let (lo, hi) = if x <= y { (x, y) } else { (y, x) };
        if hi.saturating_sub(lo) > 200 {
            return None;
        }
        Some((lo, hi))
    };

    // 第一到第十章 / 第一至第十章
    for sep in ["到第", "至第"] {
        if let Some(i) = s.find(sep) {
            let left = &s[..i];
            let right = format!("第{}", &s[i + sep.len()..]);
            if let Some(r) = pair(left, &right) {
                return Some(r);
            }
        }
    }
    // 第一到十章 / 一到十
    for sep in ["到", "至"] {
        if s.contains("到第") || s.contains("至第") {
            break;
        }
        if let Some(i) = s.find(sep) {
            if let Some(r) = pair(&s[..i], &s[i + sep.len()..]) {
                return Some(r);
            }
        }
    }
    // 第1-10章 / 1-10 / 第一章-第十章
    for sep in ["-", "–", "—", "~"] {
        if let Some(i) = s.find(sep) {
            if let Some(r) = pair(&s[..i], &s[i + sep.len()..]) {
                return Some(r);
            }
        }
    }
    let lower = raw.to_lowercase();
    if let Some(i) = lower.find(" to ") {
        let left = raw[..i].trim();
        let right = raw[i + 4..].trim();
        if let Some(r) = pair(left, right) {
            return Some(r);
        }
    }
    None
}

/// `/章节卡 1-10` · `/章节卡 第一到第十章` · `/章节卡 第 1-10 章`
fn parse_gen_chapter_cards(text: &str) -> Option<(u32, u32, Option<ChapterResolveMode>)> {
    let args = slash_args(text, &["生成章节卡", "章节卡", "chapters", "gen-chapters"])?;
    let resolve = parse_resolve_mode(args);
    if let Some((a, b)) = parse_chapter_range(args) {
        return Some((a, b, resolve));
    }
    // 单章：/章节卡 3 · /章节卡 第一章
    let single = strip_resolve_prefix(args);
    if let Some(n) = parse_chapter_ref(single) {
        return Some((n, n, resolve));
    }
    None
}

/// `/添加剧情 3 剧情正文` · `/添加剧情 第一章 剧情正文`
fn parse_add_plot_command(text: &str) -> Option<(u32, String)> {
    let args = slash_args(
        text,
        &["添加剧情", "增加剧情", "加剧情", "add-plot", "add_plot"],
    )?;
    let chapter = parse_chapter_ref(args)?;
    // 去掉开头的章号引用，剩下为剧情正文
    let rest = if let Some(i) = args.find('第') {
        if let Some(j) = args[i..].find('章') {
            let end = i + j + '章'.len_utf8();
            args[end..].trim()
        } else {
            ""
        }
    } else {
        // 阿拉伯数字或纯中文数字 token
        let token = args.split_whitespace().next().unwrap_or("");
        if token.is_empty() {
            ""
        } else if let Some(pos) = args.find(token) {
            args[pos + token.len()..]
                .trim()
                .trim_start_matches(['章', '节', '：', ':', ' ', '，', ',', '\n'])
        } else {
            ""
        }
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    Some((chapter, rest.to_string()))
}

async fn finish_chat_reply(
    state: &State<'_, AppState>,
    novel_id: &str,
    reply: String,
) -> Result<Vec<ChatMessage>, String> {
    let assistant = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.to_string(),
        role: "assistant".into(),
        content: reply,
        created_at: Utc::now().to_rfc3339(),
    };
    state
        .db
        .insert_chat(&assistant)
        .map_err(|e| e.to_string())?;
    state.db.list_chat(novel_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_novel_chat(
    state: State<'_, AppState>,
    input: NovelCreateChatInput,
) -> Result<NovelCreateChatResult, String> {
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let user_bits: Vec<&str> = input
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect();
    let loc = PromptLocale::for_interaction(&settings.ui_locale, &user_bits);
    let transcript = input
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let system = prompts::create_novel_system(loc, input.force_create);

    let user = if transcript.is_empty() {
        prompts::create_novel_empty_user(loc).to_string()
    } else {
        transcript
    };

    let model = task_model(&settings.create_model, &settings.default_model);

    let cancel = state.arm_chat_cancel(CREATE_CHAT_CANCEL_KEY);
    let _clear = ClearChatCancel {
        state: &*state,
        key: CREATE_CHAT_CANCEL_KEY.into(),
    };
    let (reply, used_mock) = llm_complete(
        &state,
        &settings,
        &system,
        &user,
        Some(&model),
        None,
        Some(cancel),
    )
    .await?;

    let mut novel = None;
    if let Some(draft) = extract_create_json(&reply) {
        let title = draft
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or(prompts::untitled_novel(loc))
            .to_string();
        let synopsis = draft
            .get("synopsis")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let strategy = draft
            .get("knowledge_strategy")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let characters = draft.get("characters").and_then(|x| x.as_array());
        let wmin = draft
            .get("word_count_min")
            .and_then(|x| x.as_u64())
            .unwrap_or(2000) as u32;
        let wmax = draft
            .get("word_count_max")
            .and_then(|x| x.as_u64())
            .unwrap_or(3000) as u32;
        let chapter_count = chapter_count_from_json(&draft)
            .or_else(|| {
                user_bits
                    .iter()
                    .rev()
                    .find_map(|t| parse_chapter_count_from_text(t))
            })
            .unwrap_or(20);
        novel = Some(create_novel_with_tree(
            &state,
            &title,
            &synopsis,
            &[],
            &strategy,
            wmin,
            wmax,
            chapter_count,
            None,
            characters.map(|v| v.as_slice()),
        )?);
    } else if input.force_create {
        // mock / 模型没吐 JSON 时兜底建一本
        let (fallback_label, fallback_syn, fallback_strat) = prompts::chat_created_novel(loc);
        let fallback_title = input
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| {
                let t: String = m.content.chars().take(24).collect();
                if t.is_empty() {
                    fallback_label.to_string()
                } else {
                    t
                }
            })
            .unwrap_or_else(|| fallback_label.to_string());
        let chapter_count = user_bits
            .iter()
            .rev()
            .find_map(|t| parse_chapter_count_from_text(t))
            .unwrap_or(20);
        novel = Some(create_novel_with_tree(
            &state,
            &fallback_title,
            fallback_syn,
            &[],
            fallback_strat,
            2000,
            3000,
            chapter_count,
            None,
            None,
        )?);
    }

    Ok(NovelCreateChatResult {
        reply,
        used_mock,
        novel,
    })
}

fn extract_create_json(reply: &str) -> Option<serde_json::Value> {
    for line in reply.lines() {
        let t = line.trim();
        if !t.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if let Some(c) = v.get("create") {
                return Some(c.clone());
            }
        }
    }
    // try whole reply
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(reply.trim()) {
        if let Some(c) = v.get("create") {
            return Some(c.clone());
        }
    }
    None
}

#[tauri::command]
pub fn get_tree(novel_id: String) -> Result<NovelTree, String> {
    let raw = fs::read_to_string(novel_tree_path(&novel_id)).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_tree(tree: NovelTree) -> Result<(), String> {
    fs::write(
        novel_tree_path(&tree.novel_id),
        serde_json::to_string_pretty(&tree).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_chapter(novel_id: String, node_id: String) -> Result<String, String> {
    let p = chapter_path(&novel_id, &node_id);
    if p.exists() {
        fs::read_to_string(p).map_err(|e| e.to_string())
    } else {
        Ok(String::new())
    }
}

/// 汇总本章大纲 + 树上链接的人物卡 / 剧情卡（edges 与 linked_* 并集）+ 相关人物关系。
fn chapter_context(tree: &NovelTree, node_id: &str, loc: PromptLocale) -> (String, String) {
    let labels = prompts::chapter_context_labels(loc);
    let node = tree.nodes.iter().find(|n| n.id == node_id);
    let outline = node.map(|n| n.outline.clone()).unwrap_or_default();
    let label = node.map(|n| n.label.clone()).unwrap_or_default();

    let mut char_ids: Vec<String> = node
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    let mut plot_ids: Vec<String> = node
        .map(|n| n.linked_side_plot_ids.clone())
        .unwrap_or_default();

    for e in &tree.edges {
        if e.source != node_id && e.target != node_id {
            continue;
        }
        let other_id = if e.source == node_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
            continue;
        };
        match other.kind {
            NodeKind::Character => {
                if !char_ids.iter().any(|id| id == other_id) {
                    char_ids.push(other_id.to_string());
                }
            }
            NodeKind::SidePlot => {
                if !plot_ids.iter().any(|id| id == other_id) {
                    plot_ids.push(other_id.to_string());
                }
            }
            _ => {}
        }
    }

    // 根节点关联人物：贯穿全书，每章上下文都纳入
    let mut root_char_ids: Vec<String> = Vec::new();
    if let Some(root) = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
    {
        for cid in &root.linked_character_ids {
            if !root_char_ids.iter().any(|id| id == cid) {
                root_char_ids.push(cid.clone());
            }
            if !char_ids.iter().any(|id| id == cid) {
                char_ids.push(cid.clone());
            }
        }
        for e in &tree.edges {
            if e.source != root.id && e.target != root.id {
                continue;
            }
            let other_id = if e.source == root.id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character) {
                if !root_char_ids.iter().any(|id| id == other_id) {
                    root_char_ids.push(other_id.to_string());
                }
                if !char_ids.iter().any(|id| id == other_id) {
                    char_ids.push(other_id.to_string());
                }
            }
        }
    }

    // 剧情卡上链接的人物也纳入本章上下文
    for pid in plot_ids.clone() {
        if let Some(plot) = tree.nodes.iter().find(|n| n.id == pid) {
            for cid in &plot.linked_character_ids {
                if !char_ids.iter().any(|id| id == cid) {
                    char_ids.push(cid.clone());
                }
            }
        }
        for e in &tree.edges {
            if e.source != pid && e.target != pid {
                continue;
            }
            let other_id = if e.source == pid {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character)
                && !char_ids.iter().any(|id| id == other_id)
            {
                char_ids.push(other_id.to_string());
            }
        }
    }

    // 本章人物若与另一人物有关系线，把对方一并纳入上下文
    let seed = char_ids.clone();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        let a_in = seed.iter().any(|id| id == &e.source);
        let b_in = seed.iter().any(|id| id == &e.target);
        if !a_in && !b_in {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) {
            char_ids.push(e.source.clone());
        }
        if !char_ids.iter().any(|id| id == &e.target) {
            char_ids.push(e.target.clone());
        }
    }

    let mut characters = String::new();
    for id in &char_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let bookwide = if root_char_ids.iter().any(|r| r == id) {
                if loc.is_zh() {
                    "｜全书人物"
                } else {
                    "｜book-wide"
                }
            } else {
                ""
            };
            if let Some(c) = &n.character {
                characters.push_str(&format!(
                    "- {}：{}{}｜{}：{}｜{}：{}｜{}：{}\n  {}：{}\n  {}：{}\n  {}：{}\n",
                    labels.name,
                    n.label,
                    bookwide,
                    labels.role,
                    c.role,
                    labels.gender,
                    c.gender,
                    labels.alignment,
                    c.alignment,
                    labels.personality,
                    c.personality,
                    labels.style,
                    c.style,
                    labels.motto,
                    c.motto
                ));
            } else {
                characters.push_str(&format!(
                    "- {}：{}{} {}\n",
                    labels.name, n.label, bookwide, labels.card_incomplete
                ));
            }
        }
    }
    if characters.is_empty() {
        characters.push_str(labels.no_characters);
    }

    let mut relations = String::new();
    for e in &tree.edges {
        let a = tree.nodes.iter().find(|n| n.id == e.source);
        let b = tree.nodes.iter().find(|n| n.id == e.target);
        let (Some(a), Some(b)) = (a, b) else { continue };
        if !matches!(a.kind, NodeKind::Character) || !matches!(b.kind, NodeKind::Character) {
            continue;
        }
        if !char_ids.iter().any(|id| id == &e.source) || !char_ids.iter().any(|id| id == &e.target)
        {
            continue;
        }
        let rel = if e.label.trim().is_empty() {
            labels.relation_unset
        } else {
            e.label.as_str()
        };
        relations.push_str(&format!("- {} ↔ {}：{}\n", a.label, b.label, rel));
    }

    let mut plots = String::new();
    for id in &plot_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            let outline_text = if n.outline.trim().is_empty() {
                labels.empty_plot
            } else {
                n.outline.as_str()
            };
            plots.push_str(&format!("- 「{}」：{}\n", n.label, outline_text));
        }
    }
    if plots.is_empty() {
        plots.push_str(labels.no_plots);
    }

    let outline_text = if outline.trim().is_empty() {
        labels.empty_outline.to_string()
    } else {
        outline
    };

    let chapter_info = format!(
        "{}：{label}\n{}：\n{outline_text}",
        labels.chapter_title, labels.chapter_outline
    );
    let cards = if relations.is_empty() {
        format!(
            "{}\n{characters}\n{}\n{plots}",
            labels.chars_header, labels.plots_header
        )
    } else {
        format!(
            "{}\n{characters}\n{}\n{relations}\n{}\n{plots}",
            labels.chars_header, labels.relations_header, labels.plots_header
        )
    };
    (chapter_info, cards)
}

/// 预生成用：根节点故事简介 + 根节点关联人物卡。
fn root_generate_reference(tree: &NovelTree, synopsis: &str, loc: PromptLocale) -> String {
    let labels = prompts::chapter_context_labels(loc);
    let root = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel));
    let root_outline = root.map(|n| n.outline.as_str()).unwrap_or("");
    let root_id = root.map(|n| n.id.as_str()).unwrap_or("");

    let mut char_ids: Vec<String> = root
        .map(|n| n.linked_character_ids.clone())
        .unwrap_or_default();
    if !root_id.is_empty() {
        for e in &tree.edges {
            if e.source != root_id && e.target != root_id {
                continue;
            }
            let other_id = if e.source == root_id {
                e.target.as_str()
            } else {
                e.source.as_str()
            };
            let Some(other) = tree.nodes.iter().find(|n| n.id == other_id) else {
                continue;
            };
            if matches!(other.kind, NodeKind::Character)
                && !char_ids.iter().any(|id| id == other_id)
            {
                char_ids.push(other_id.to_string());
            }
        }
    }

    let mut characters = String::new();
    for id in &char_ids {
        if let Some(n) = tree.nodes.iter().find(|x| x.id == *id) {
            if let Some(c) = &n.character {
                characters.push_str(&format!(
                    "- {}：{}｜{}：{}｜{}：{}｜{}：{}\n  {}：{}\n  {}：{}\n  {}：{}\n",
                    labels.name,
                    n.label,
                    labels.role,
                    c.role,
                    labels.gender,
                    c.gender,
                    labels.alignment,
                    c.alignment,
                    labels.personality,
                    c.personality,
                    labels.style,
                    c.style,
                    labels.motto,
                    c.motto
                ));
            } else {
                characters.push_str(&format!(
                    "- {}：{} {}\n",
                    labels.name, n.label, labels.card_incomplete
                ));
            }
        }
    }
    if characters.is_empty() {
        characters.push_str(if loc.is_zh() {
            "（根节点未关联人物卡）\n"
        } else {
            "(no characters linked to root)\n"
        });
    }

    let syn = if synopsis.trim().is_empty() {
        if loc.is_zh() {
            "（暂无简介）"
        } else {
            "(no synopsis)"
        }
    } else {
        synopsis
    };
    let extra = if root_outline.trim().is_empty() {
        String::new()
    } else if loc.is_zh() {
        format!("根节点补充说明：\n{root_outline}\n")
    } else {
        format!("Root notes:\n{root_outline}\n")
    };

    if loc.is_zh() {
        format!("【全书故事简介】\n{syn}\n{extra}\n【根节点关联人物卡（参考）】\n{characters}")
    } else {
        format!("[Novel synopsis]\n{syn}\n{extra}\n[Root-linked character cards (reference)]\n{characters}")
    }
}

#[tauri::command]
pub async fn generate_chapter(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
) -> Result<GenerateResult, String> {
    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let node_outline = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.outline.as_str())
        .unwrap_or("");
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[novel.title.as_str(), novel.synopsis.as_str(), node_outline],
    );
    let root_ref = root_generate_reference(&tree, &novel.synopsis, loc);
    let (chapter_info, cards) = chapter_context(&tree, &node_id, loc);
    let system = prompts::generate_chapter_system(
        loc,
        &novel.title,
        &novel.synopsis,
        &novel.knowledge_strategy,
        novel.word_count_min,
        novel.word_count_max,
        novel.chapter_count,
    );
    let user = prompts::generate_chapter_user(
        loc,
        &root_ref,
        &chapter_info,
        &cards,
        novel.word_count_min,
        novel.word_count_max,
    );
    let model = task_model(&settings.generate_model, &settings.default_model);
    let (mut content, used_mock) = llm_complete(
        &state,
        &settings,
        &system,
        &user,
        Some(&model),
        Some(&novel_id),
        None,
    )
    .await
    .map_err(|e| e.to_string())?;
    content.push_str(&prompts::generate_footer(loc, &node_id, &model));
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let words = count_words(&content);
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    Ok(GenerateResult {
        content,
        used_mock,
        message: prompts::generate_done_msg(loc, &model, words, used_mock),
    })
}

#[tauri::command]
pub async fn refine_chapter(
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    prev_n: u32,
) -> Result<GenerateResult, String> {
    let tree = get_tree(novel_id.clone())?;
    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let node_outline = tree
        .nodes
        .iter()
        .find(|n| n.id == node_id)
        .map(|n| n.outline.as_str())
        .unwrap_or("");
    let current = get_chapter(novel_id.clone(), node_id.clone())?;
    if current.trim().is_empty() {
        return Err("当前章尚无正文，请先生成再精修".into());
    }
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            novel.title.as_str(),
            novel.synopsis.as_str(),
            node_outline,
            current.as_str(),
        ],
    );
    let (chapter_info, cards) = chapter_context(&tree, &node_id, loc);

    // previous chapters along chapter chain order by y
    let mut chapters: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .cloned()
        .collect();
    chapters.sort_by(|a, b| {
        a.position
            .y
            .partial_cmp(&b.position.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let idx = chapters.iter().position(|c| c.id == node_id);
    let mut prev_text = String::new();
    let mut linked_n = 0u32;
    if let Some(i) = idx {
        let start = i.saturating_sub(prev_n as usize);
        linked_n = (i - start) as u32;
        for c in &chapters[start..i] {
            let body = get_chapter(novel_id.clone(), c.id.clone()).unwrap_or_default();
            prev_text.push_str(&prompts::prev_chapter_block(
                loc, &c.label, &c.outline, &body,
            ));
        }
    }

    let refine_model = task_model(&settings.refine_model, &settings.default_model);
    let system = prompts::refine_chapter_system(
        loc,
        &novel.title,
        linked_n,
        novel.word_count_min,
        novel.word_count_max,
    );
    let user = prompts::refine_chapter_user(
        loc,
        &novel.knowledge_strategy,
        linked_n,
        &prev_text,
        &chapter_info,
        &cards,
        &current,
    );
    let (content, used_mock) = llm_complete(
        &state,
        &settings,
        &system,
        &user,
        Some(&refine_model),
        Some(&novel_id),
        None,
    )
    .await
    .map_err(|e| e.to_string())?;
    fs::write(chapter_path(&novel_id, &node_id), &content).map_err(|e| e.to_string())?;
    let words = count_words(&content);
    let mut tree = get_tree(novel_id.clone())?;
    set_node_word_count(&mut tree, &node_id, words);
    let _ = save_tree(tree);
    Ok(GenerateResult {
        content,
        used_mock,
        message: prompts::refine_done_msg(loc, &refine_model, linked_n, words, used_mock),
    })
}

#[tauri::command]
pub fn list_chat_messages(
    state: State<'_, AppState>,
    novel_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state.db.list_chat(&novel_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    content: String,
) -> Result<Vec<ChatMessage>, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };

    let now = Utc::now().to_rfc3339();
    let user_msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.clone(),
        role: "user".into(),
        content: content.clone(),
        created_at: now.clone(),
    };
    state.db.insert_chat(&user_msg).map_err(|e| e.to_string())?;

    emit_chat_progress(&app, &novel_id, "context");
    let mut tree = get_tree(novel_id.clone())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            content.as_str(),
            novel.title.as_str(),
            novel.synopsis.as_str(),
        ],
    );

    // —— 根 Chat 明确指令（确定性，优先于自由对话）——
    if parse_clear_all_chapters(&content) {
        emit_chat_progress(&app, &novel_id, "apply_outlines");
        let n = tree
            .nodes
            .iter()
            .filter(|x| matches!(x.kind, NodeKind::Chapter))
            .count();
        clear_all_chapter_nodes(&mut tree);
        let _ = save_tree(tree);
        emit_chat_progress(&app, &novel_id, "saving");
        let reply = if loc.is_zh() {
            format!("已清空全部章节节点（共 {n} 个）。角色卡与剧情卡仍保留。")
        } else {
            format!("Cleared all chapter nodes ({n}). Character/plot cards kept.")
        };
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some((ch, plot)) = parse_add_plot_command(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        let reply = match add_plot_to_chapter(&mut tree, ch, &plot) {
            Ok(msg) => {
                let _ = save_tree(tree);
                msg
            }
            Err(e) => e,
        };
        emit_chat_progress(&app, &novel_id, "saving");
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some(chapter_num) = parse_gen_plots_for_chapter(&content) {
        let existing = existing_chapter_nums(&tree);
        let Some(chapter_id) = existing.get(&chapter_num).cloned() else {
            let reply = if loc.is_zh() {
                format!("结构树上找不到第{chapter_num}章，请先生成章节卡。")
            } else {
                format!("Chapter {chapter_num} not found. Generate the chapter card first.")
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        };
        let reply = enrich_plots_for_chapter_node(
            &app,
            &state,
            &settings,
            &novel,
            &novel_id,
            &mut tree,
            &chapter_id,
            Some(chapter_num),
            loc,
            Some(cancel.clone()),
        )
        .await?;
        return finish_chat_reply(&state, &novel_id, reply).await;
    }

    if let Some((from, to, resolve)) = parse_gen_chapter_cards(&content) {
        let existing = existing_chapter_nums(&tree);
        let conflicts: Vec<u32> = (from..=to).filter(|n| existing.contains_key(n)).collect();
        if !conflicts.is_empty() && resolve.is_none() {
            let list = conflicts
                .iter()
                .map(|n| format!("第{n}章"))
                .collect::<Vec<_>>()
                .join("、");
            let reply = if loc.is_zh() {
                format!(
                    "章节编号冲突：{list} 已在结构树上。\n\
                     请用带模式的指令重试：\n\
                     · `/章节卡 覆盖 {from}-{to}`\n\
                     · `/章节卡 跳过 {from}-{to}`\n\
                     · `/章节卡 强制追加 {from}-{to}`\n\
                     或先 `/清空章节` 再生成。"
                )
            } else {
                format!(
                    "Chapter number conflict: {list} already on the tree.\n\
                     Retry with:\n\
                     · `/chapters overwrite {from}-{to}`\n\
                     · `/chapters skip {from}-{to}`\n\
                     · `/chapters force {from}-{to}`\n\
                     Or `/clear-chapters` first."
                )
            };
            emit_chat_progress(&app, &novel_id, "saving");
            return finish_chat_reply(&state, &novel_id, reply).await;
        }
        let mode = resolve.unwrap_or(ChapterResolveMode::ForceAppend);
        let nums: Vec<u32> = match mode {
            ChapterResolveMode::Skip => (from..=to).filter(|n| !existing.contains_key(n)).collect(),
            ChapterResolveMode::Overwrite | ChapterResolveMode::ForceAppend => {
                (from..=to).collect()
            }
        };
        if nums.is_empty() {
            let reply = if loc.is_zh() {
                "指定范围内没有需要新生成的章节（均已存在且选择了跳过）。".into()
            } else {
                "Nothing to generate in that range (all skipped).".into()
            };
            return finish_chat_reply(&state, &novel_id, reply).await;
        }
        emit_chat_progress(&app, &novel_id, "thinking");
        let chat_model = task_model(&settings.chat_model, &settings.default_model);
        let system = prompts::gen_chapter_cards_system(
            loc,
            &novel.title,
            &novel.synopsis,
            novel.chapter_count,
            from,
            to,
            &nums,
        );
        let user = prompts::gen_chapter_cards_user(loc, from, to, &nums);
        let (reply, _) = llm_complete(
            &state,
            &settings,
            &system,
            &user,
            Some(&chat_model),
            Some(&novel_id),
            Some(cancel.clone()),
        )
        .await?;
        let mut applied = false;
        for line in reply.lines() {
            let line = line.trim();
            if !line.starts_with('{') {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(list) = v.get("outlines").and_then(|x| x.as_array()) {
                if !list.is_empty() {
                    emit_chat_progress(&app, &novel_id, "apply_outlines");
                    apply_chapter_outlines(&mut tree, list, mode);
                    applied = true;
                    break;
                }
            }
        }
        if applied {
            let _ = save_tree(tree);
        }
        emit_chat_progress(&app, &novel_id, "saving");
        let mode_zh = match mode {
            ChapterResolveMode::Overwrite => "覆盖",
            ChapterResolveMode::Skip => "跳过已有",
            ChapterResolveMode::ForceAppend => "追加",
        };
        let note = if applied {
            if loc.is_zh() {
                format!("\n\n（已按「{mode_zh}」写入第{from}–{to}章大纲到结构树。）")
            } else {
                format!("\n\n(Applied chapters {from}–{to}, mode={mode_zh}.)")
            }
        } else if loc.is_zh() {
            "\n\n（未解析到 outlines JSON，结构树未改动。请重试。）".into()
        } else {
            "\n\n(No outlines JSON found; tree unchanged.)".into()
        };
        return finish_chat_reply(&state, &novel_id, format!("{reply}{note}")).await;
    }

    // 以 / 开头但未识别 → 列出指令，不走自由对话
    if content.trim().starts_with('/') {
        emit_chat_progress(&app, &novel_id, "saving");
        return finish_chat_reply(&state, &novel_id, root_slash_help(loc.is_zh())).await;
    }

    let has_chapters = tree
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Chapter));
    let chapter_list = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .map(|n| format!("{}({})", n.label, n.id))
        .collect::<Vec<_>>()
        .join(", ");
    let system = prompts::workspace_chat_system(
        loc,
        &novel.title,
        &novel.synopsis,
        has_chapters,
        novel.word_count_min,
        novel.word_count_max,
        novel.chapter_count,
        &chapter_list,
    );

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (reply, _) = llm_complete(
        &state,
        &settings,
        &system,
        &content,
        Some(&chat_model),
        Some(&novel_id),
        Some(cancel.clone()),
    )
    .await?;

    let (def_label, def_role, def_align) = prompts::default_new_character(loc);
    let mut tree_dirty = false;

    // 用户话里直接改字数 / 总章数 → 立刻落库（不依赖模型 JSON）
    if let Some((wmin, wmax)) = parse_word_target_from_text(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        apply_novel_word_target(&state, &mut novel, &mut tree, wmin, wmax)?;
        tree_dirty = true;
    }
    if let Some(n) = parse_chapter_count_from_text(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        apply_novel_chapter_count(&state, &mut novel, &mut tree, n)?;
        tree_dirty = true;
    }

    for line in reply.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some((wmin, wmax)) = word_target_from_json(&v) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            apply_novel_word_target(&state, &mut novel, &mut tree, wmin, wmax)?;
            tree_dirty = true;
        }
        if let Some(n) = chapter_count_from_json(&v) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            apply_novel_chapter_count(&state, &mut novel, &mut tree, n)?;
            tree_dirty = true;
        }
        if let Some(c) = v.get("character") {
            emit_chat_progress(&app, &novel_id, "apply_character");
            let cid = uuid::Uuid::new_v4().to_string();
            let label = c
                .get("label")
                .and_then(|x| x.as_str())
                .unwrap_or(def_label)
                .to_string();
            let y = tree
                .nodes
                .iter()
                .map(|n| n.position.y)
                .fold(0.0_f64, f64::max)
                + 120.0;
            tree.nodes.push(TreeNode {
                id: cid.clone(),
                kind: NodeKind::Character,
                label,
                outline: String::new(),
                character: Some(CharacterCard {
                    role: c
                        .get("role")
                        .and_then(|x| x.as_str())
                        .unwrap_or(def_role)
                        .into(),
                    personality: c
                        .get("personality")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    motto: c.get("motto").and_then(|x| x.as_str()).unwrap_or("").into(),
                    gender: c
                        .get("gender")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .into(),
                    style: c.get("style").and_then(|x| x.as_str()).unwrap_or("").into(),
                    alignment: c
                        .get("alignment")
                        .and_then(|x| x.as_str())
                        .unwrap_or(def_align)
                        .into(),
                }),
                linked_character_ids: vec![],
                linked_side_plot_ids: vec![],
                position: NodePosition { x: 40.0, y },
                word_count: 0,
                word_count_min: 0,
                word_count_max: 0,
                chapter_count: 0,
            });
            let link = c
                .get("link_chapter_id")
                .and_then(|x| x.as_str())
                .filter(|id| tree.nodes.iter().any(|n| n.id == *id))
                .map(|s| s.to_string())
                .or_else(|| {
                    tree.nodes
                        .iter()
                        .find(|n| matches!(n.kind, NodeKind::Novel))
                        .map(|n| n.id.clone())
                });
            if let Some(ch) = link {
                if let Some(node) = tree.nodes.iter_mut().find(|n| n.id == ch) {
                    node.linked_character_ids.push(cid.clone());
                }
                tree.edges.push(TreeEdge {
                    id: format!("e-{ch}-{cid}"),
                    source: ch,
                    target: cid,
                    kind: "character".into(),
                    source_handle: Some("left".into()),
                    target_handle: Some("right".into()),
                    label: String::new(),
                });
            }
            tree_dirty = true;
        }
        if let Some(list) = v.get("outlines").and_then(|x| x.as_array()) {
            if !list.is_empty() {
                emit_chat_progress(&app, &novel_id, "apply_outlines");
                // 自由对话产出的大纲：追加，避免误清空
                apply_chapter_outlines(&mut tree, list, ChapterResolveMode::ForceAppend);
                tree_dirty = true;
            }
        }
    }
    if tree_dirty {
        let _ = save_tree(tree);
    }

    emit_chat_progress(&app, &novel_id, "saving");
    finish_chat_reply(&state, &novel_id, reply).await
}

#[tauri::command]
pub async fn card_chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    content: String,
) -> Result<CardChatResult, String> {
    let cancel = state.arm_chat_cancel(&novel_id);
    let _clear = ClearChatCancel {
        state: &*state,
        key: novel_id.clone(),
    };

    emit_chat_progress(&app, &novel_id, "context");
    let mut tree = get_tree(novel_id.clone())?;
    let idx = tree
        .nodes
        .iter()
        .position(|n| n.id == node_id)
        .ok_or_else(|| "节点不存在".to_string())?;
    let kind = tree.nodes[idx].kind.clone();
    if matches!(kind, NodeKind::Novel) {
        return Err("根节点请用创作 Chat".into());
    }

    let novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let settings = state.db.get_settings().map_err(|e| e.to_string())?;
    let node_label = tree.nodes[idx].label.clone();
    let node_outline = tree.nodes[idx].outline.clone();
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            content.as_str(),
            novel.title.as_str(),
            node_label.as_str(),
            node_outline.as_str(),
        ],
    );
    if matches!(kind, NodeKind::Chapter) && parse_enrich_plots_command(&content) {
        let reply = enrich_plots_for_chapter_node(
            &app,
            &state,
            &settings,
            &novel,
            &novel_id,
            &mut tree,
            &node_id,
            chapter_number_from_label(&node_label),
            loc,
            Some(cancel.clone()),
        )
        .await?;
        let tree = get_tree(novel_id)?;
        return Ok(CardChatResult { reply, tree });
    }

    let kind_key = match kind {
        NodeKind::Chapter => "chapter",
        NodeKind::Character => "character",
        NodeKind::SidePlot => "side_plot",
        NodeKind::Novel => unreachable!(),
    };
    let system = prompts::card_chat_system(loc, kind_key, &novel.title, &node_label, &node_outline);

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (reply, _) = llm_complete(
        &state,
        &settings,
        &system,
        &content,
        Some(&chat_model),
        Some(&novel_id),
        Some(cancel),
    )
    .await?;

    if let Some(line) = reply.lines().find(|l| l.trim().starts_with('{')) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            emit_chat_progress(&app, &novel_id, "apply_card");
            let n = &mut tree.nodes[idx];
            match kind {
                NodeKind::Chapter | NodeKind::SidePlot => {
                    if let Some(label) = v.get("label").and_then(|x| x.as_str()) {
                        if !label.is_empty() {
                            n.label = label.to_string();
                        }
                    }
                    if let Some(outline) = v.get("outline").and_then(|x| x.as_str()) {
                        n.outline = outline.to_string();
                    }
                }
                NodeKind::Character => {
                    if let Some(c) = v.get("character") {
                        if let Some(label) = c.get("label").and_then(|x| x.as_str()) {
                            if !label.is_empty() {
                                n.label = label.to_string();
                            }
                        }
                        let mut card = n.character.clone().unwrap_or(CharacterCard {
                            role: String::new(),
                            personality: String::new(),
                            motto: String::new(),
                            gender: String::new(),
                            style: String::new(),
                            alignment: String::new(),
                        });
                        if let Some(s) = c.get("role").and_then(|x| x.as_str()) {
                            card.role = s.into();
                        }
                        if let Some(s) = c.get("personality").and_then(|x| x.as_str()) {
                            card.personality = s.into();
                        }
                        if let Some(s) = c.get("motto").and_then(|x| x.as_str()) {
                            card.motto = s.into();
                        }
                        if let Some(s) = c.get("gender").and_then(|x| x.as_str()) {
                            card.gender = s.into();
                        }
                        if let Some(s) = c.get("style").and_then(|x| x.as_str()) {
                            card.style = s.into();
                        }
                        if let Some(s) = c.get("alignment").and_then(|x| x.as_str()) {
                            card.alignment = s.into();
                        }
                        n.character = Some(card);
                    }
                }
                NodeKind::Novel => {}
            }
        }
    }

    emit_chat_progress(&app, &novel_id, "saving");
    save_tree(tree.clone())?;
    Ok(CardChatResult { reply, tree })
}

#[tauri::command]
pub async fn pick_cover(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
        .blocking_pick_file();
    Ok(file.and_then(|f| f.as_path().map(|p| p.to_string_lossy().to_string())))
}

#[tauri::command]
pub fn set_cover(
    state: State<'_, AppState>,
    novel_id: String,
    source_path: String,
) -> Result<NovelProject, String> {
    let mut novel = state
        .db
        .get_novel(&novel_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "小说不存在".to_string())?;
    let ext = Path::new(&source_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let dest = novel_dir(&novel_id).join(format!("cover.{ext}"));
    fs::copy(&source_path, &dest).map_err(|e| e.to_string())?;
    novel.cover_path = Some(dest.to_string_lossy().to_string());
    novel.updated_at = Utc::now().to_rfc3339();
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    fs::write(
        novel_meta_path(&novel_id),
        serde_json::to_string_pretty(&novel).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(novel)
}

#[tauri::command]
pub async fn pick_text_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Text", &["txt"])
        .add_filter("All", &["txt", "epub", "pdf"])
        .blocking_pick_file();
    Ok(file.and_then(|f| f.as_path().map(|p| p.to_string_lossy().to_string())))
}

#[tauri::command]
pub fn app_data_root() -> String {
    app_root().to_string_lossy().to_string()
}

#[tauri::command]
pub fn get_token_usage(state: State<'_, AppState>, date: String) -> Result<TokenUsageDay, String> {
    let day = date.trim();
    let day = if day.is_empty() {
        Local::now().format("%Y-%m-%d").to_string()
    } else {
        day.to_string()
    };
    let rows = state
        .db
        .list_token_usage_day(&day)
        .map_err(|e| e.to_string())?;
    let novels = state
        .db
        .list_token_usage_by_novel(&day)
        .map_err(|e| e.to_string())?;
    let mut models: Vec<String> = rows.iter().map(|r| r.model.clone()).collect();
    models.sort();
    models.dedup();
    Ok(TokenUsageDay {
        date: day,
        rows,
        models,
        novels,
    })
}

#[tauri::command]
pub fn get_token_usage_month(
    state: State<'_, AppState>,
    month: String,
) -> Result<TokenUsageMonth, String> {
    let m = month.trim();
    let m = if m.is_empty() {
        Local::now().format("%Y-%m").to_string()
    } else {
        m.to_string()
    };
    let models = state
        .db
        .list_token_usage_month(&m)
        .map_err(|e| e.to_string())?;
    let total_tokens = models.iter().map(|r| r.total_tokens).sum();
    Ok(TokenUsageMonth {
        month: m,
        total_tokens,
        models,
    })
}

pub fn init_state() -> Result<AppState, String> {
    let db = Db::open().map_err(|e| e.to_string())?;
    crate::sample::ensure_sample(&db).map_err(|e| e.to_string())?;
    Ok(AppState {
        db: Arc::new(db),
        chat_cancel: Default::default(),
    })
}

#[tauri::command]
pub fn chat_cancel(state: State<'_, AppState>, novel_id: String) -> Result<(), String> {
    let key = if novel_id.trim().is_empty() {
        CREATE_CHAT_CANCEL_KEY
    } else {
        novel_id.trim()
    };
    state.request_chat_cancel(key);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::PromptLocale;

    #[test]
    fn parse_single_chapter_word_target() {
        assert_eq!(
            parse_word_target_from_text("请把每章目标改成4500字"),
            Some((4500, 4500))
        );
        assert_eq!(
            parse_word_target_from_text("set per chapter target to 4500 words"),
            Some((4500, 4500))
        );
    }

    #[test]
    fn parse_range_chapter_word_target() {
        assert_eq!(
            parse_word_target_from_text("每章 4000-4500 字"),
            Some((4000, 4500))
        );
    }

    #[test]
    fn word_target_json() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"word_count":{"min":4500,"max":4500}}"#).unwrap();
        assert_eq!(word_target_from_json(&v), Some((4500, 4500)));
    }

    #[test]
    fn parse_planned_chapter_count() {
        assert_eq!(parse_chapter_count_from_text("这本一共20章"), Some(20));
        assert_eq!(parse_chapter_count_from_text("计划总共 30 章"), Some(30));
        assert_eq!(
            parse_chapter_count_from_text("plan about 25 chapters total"),
            Some(25)
        );
        let v: serde_json::Value = serde_json::from_str(r#"{"chapter_count":40}"#).unwrap();
        assert_eq!(chapter_count_from_json(&v), Some(40));
    }

    #[test]
    fn detect_zh_over_english_ui() {
        let loc = PromptLocale::for_interaction("en", &["帮我写一个腹黑女主，每章4500字"]);
        assert!(loc.is_zh());
    }

    #[test]
    fn detect_en_input() {
        let loc = PromptLocale::for_interaction(
            "zh-CN",
            &["Write a calm heroine and set 4500 words per chapter"],
        );
        assert_eq!(loc, PromptLocale::En);
    }

    #[test]
    fn parse_root_chat_commands() {
        assert!(parse_clear_all_chapters("/清空章节"));
        assert!(!parse_clear_all_chapters("清空所有章节节点"));
        assert_eq!(parse_gen_chapter_cards("/章节卡 1-10"), Some((1, 10, None)));
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第 1-10 章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第1-10章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第一到第十章"),
            Some((1, 10, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 第一章到第十二章"),
            Some((1, 12, None))
        );
        assert_eq!(
            parse_gen_chapter_cards("/章节卡 覆盖 3-5"),
            Some((3, 5, Some(ChapterResolveMode::Overwrite)))
        );
        assert_eq!(
            parse_add_plot_command("/添加剧情 3 主角发现密信"),
            Some((3, "主角发现密信".into()))
        );
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 3"), Some(3));
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 第一章"), Some(1));
        assert_eq!(parse_gen_plots_for_chapter("/剧情卡 第十二章"), Some(12));
        assert!(parse_enrich_plots_command("/完善剧情"));
        assert!(parse_enrich_plots_command("/enrich-plots"));
        assert!(!parse_enrich_plots_command("/剧情卡 3"));
        assert_eq!(parse_gen_chapter_cards("/剧情卡 3"), None);
        assert_eq!(parse_gen_chapter_cards("生成 1-10 章的章节卡"), None);
        assert_eq!(chapter_number_from_label("第一章 · 夜色"), Some(1));
        assert_eq!(chapter_number_from_label("第十二章"), Some(12));
        assert_eq!(chapter_number_from_label("第12章 · 夜色"), Some(12));
        assert_eq!(parse_chinese_numeral("二十一"), Some(21));
        assert_eq!(
            parse_add_plot_command("/添加剧情 第一章 主角发现密信"),
            Some((1, "主角发现密信".into()))
        );
    }
}
