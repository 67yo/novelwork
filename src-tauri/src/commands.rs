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
use std::sync::Arc;
use tauri::{Emitter, State};
use tauri_plugin_dialog::DialogExt;

fn emit_chat_progress(app: &tauri::AppHandle, novel_id: &str, step: &str) {
    let _ = app.emit(
        "chat-progress",
        serde_json::json!({ "novelId": novel_id, "step": step }),
    );
}

/// LLM 调用并按本地小时累计 token。
async fn llm_complete(
    state: &State<'_, AppState>,
    settings: &AppSettings,
    system: &str,
    user: &str,
    model: Option<&str>,
) -> Result<(String, bool), String> {
    let r = llm::complete(settings, system, user, model)
        .await
        .map_err(|e| e.to_string())?;
    let now = Local::now();
    let day = now.format("%Y-%m-%d").to_string();
    let hour = now.hour() as u8;
    let _ = state.db.add_token_usage(
        &day,
        hour,
        &r.model,
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
pub async fn refresh_model_catalog(
    state: State<'_, AppState>,
) -> Result<ModelCatalog, String> {
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
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[&extract_prompt, &excerpt],
    );
    if !settings.deepseek_api_key.trim().is_empty() && !extract_prompt.trim().is_empty() {
        let sys = prompts::knowledge_extract_system(loc);
        let user = prompts::knowledge_extract_user(loc, &extract_prompt, &excerpt);
        if let Ok((out, _)) = llm_complete(&state, &settings, sys, &user, None).await {
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
    let min = v.get("word_count_min").and_then(|x| x.as_u64()).map(|x| x as u32);
    let max = v.get("word_count_max").and_then(|x| x.as_u64()).map(|x| x as u32);
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
    _chapters: Option<&[serde_json::Value]>,
    characters: Option<&[serde_json::Value]>,
) -> Result<NovelProject, String> {
    let now = Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let (wmin, wmax) = normalize_word_range(word_count_min, word_count_max);
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
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_novel(&novel).map_err(|e| e.to_string())?;
    fs::write(
        novel_meta_path(&id),
        serde_json::to_string_pretty(&novel).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let tree = build_initial_tree(&id, title, synopsis, wmin, wmax, characters);
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
                    gender: c.get("gender").and_then(|x| x.as_str()).unwrap_or("").into(),
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

/// 将大纲挂到树上：替换已有章节链，保留根节点与角色/支线。
fn apply_chapter_outlines(tree: &mut NovelTree, list: &[serde_json::Value]) {
    let root_id = tree
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Novel))
        .map(|n| n.id.clone())
        .unwrap_or_else(|| "root".into());

    let old_chapter_ids: Vec<String> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .map(|n| n.id.clone())
        .collect();
    tree.nodes
        .retain(|n| !matches!(n.kind, NodeKind::Chapter));
    tree.edges.retain(|e| {
        e.kind != "chapter"
            && !old_chapter_ids.iter().any(|id| e.source == *id || e.target == *id)
    });
    for n in tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Character))
        .map(|n| n.id.clone())
        .collect::<Vec<_>>()
    {
        let linked = tree
            .edges
            .iter()
            .any(|e| e.target == n && e.kind == "character");
        if !linked {
            if let Some(root) = tree.nodes.iter_mut().find(|x| x.id == root_id) {
                if !root.linked_character_ids.contains(&n) {
                    root.linked_character_ids.push(n.clone());
                }
            }
            tree.edges.push(TreeEdge {
                id: format!("e-{root_id}-{n}"),
                source: root_id.clone(),
                target: n,
                kind: "character".into(),
                source_handle: Some("left".into()),
                target_handle: Some("right".into()),
                label: String::new(),
            });
        }
    }

    let mut prev = root_id;
    for (i, c) in list.iter().enumerate() {
        let cid = format!("c-{}", i + 1);
        let label = c
            .get("label")
            .or_else(|| c.get("title"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("第{}章", i + 1));
        let outline = c
            .get("outline")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let y = 140.0 * (i as f64 + 1.0);
        tree.nodes.push(TreeNode {
            id: cid.clone(),
            kind: NodeKind::Chapter,
            label,
            outline,
            character: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            position: NodePosition { x: 280.0, y },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
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
        prev = cid;
    }
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

    let (reply, used_mock) = llm_complete(&state, &settings, &system, &user, Some(&model))
        .await
        .map_err(|e| e.to_string())?;

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
        novel = Some(create_novel_with_tree(
            &state,
            &title,
            &synopsis,
            &[],
            &strategy,
            wmin,
            wmax,
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
        novel = Some(create_novel_with_tree(
            &state,
            &fallback_title,
            fallback_syn,
            &[],
            fallback_strat,
            2000,
            3000,
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
            if matches!(other.kind, NodeKind::Character) && !char_ids.iter().any(|id| id == other_id)
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
    let (chapter_info, cards) = chapter_context(&tree, &node_id, loc);
    let system = prompts::generate_chapter_system(
        loc,
        &novel.title,
        &novel.synopsis,
        &novel.knowledge_strategy,
        novel.word_count_min,
        novel.word_count_max,
    );
    let user = prompts::generate_chapter_user(
        loc,
        &chapter_info,
        &cards,
        novel.word_count_min,
        novel.word_count_max,
    );
    let model = task_model(&settings.generate_model, &settings.default_model);
    let (mut content, used_mock) = llm_complete(&state, &settings, &system, &user, Some(&model))
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
    chapters.sort_by(|a, b| a.position.y.partial_cmp(&b.position.y).unwrap());
    let idx = chapters.iter().position(|c| c.id == node_id);
    let mut prev_text = String::new();
    let mut linked_n = 0u32;
    if let Some(i) = idx {
        let start = i.saturating_sub(prev_n as usize);
        linked_n = (i - start) as u32;
        for c in &chapters[start..i] {
            let body = get_chapter(novel_id.clone(), c.id.clone()).unwrap_or_default();
            prev_text.push_str(&prompts::prev_chapter_block(loc, &c.label, &c.outline, &body));
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
    let (content, used_mock) = llm_complete(&state, &settings, &system, &user, Some(&refine_model))
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
        &[content.as_str(), novel.title.as_str(), novel.synopsis.as_str()],
    );

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
        &chapter_list,
    );

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (reply, _) = llm_complete(&state, &settings, &system, &content, Some(&chat_model))
        .await
        .map_err(|e| e.to_string())?;

    let (def_label, def_role, def_align) = prompts::default_new_character(loc);
    let mut tree_dirty = false;

    // 用户话里直接改字数 → 立刻落库（不依赖模型 JSON）
    if let Some((wmin, wmax)) = parse_word_target_from_text(&content) {
        emit_chat_progress(&app, &novel_id, "apply_card");
        apply_novel_word_target(&state, &mut novel, &mut tree, wmin, wmax)?;
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
                        gender: c.get("gender").and_then(|x| x.as_str()).unwrap_or("").into(),
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
                apply_chapter_outlines(&mut tree, list);
                tree_dirty = true;
            }
        }
    }
    if tree_dirty {
        let _ = save_tree(tree);
    }

    emit_chat_progress(&app, &novel_id, "saving");
    let assistant = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        novel_id: novel_id.clone(),
        role: "assistant".into(),
        content: reply,
        created_at: Utc::now().to_rfc3339(),
    };
    state.db.insert_chat(&assistant).map_err(|e| e.to_string())?;
    state.db.list_chat(&novel_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn card_chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    novel_id: String,
    node_id: String,
    content: String,
) -> Result<CardChatResult, String> {
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
    let node = &tree.nodes[idx];
    let loc = PromptLocale::for_interaction(
        &settings.ui_locale,
        &[
            content.as_str(),
            novel.title.as_str(),
            node.label.as_str(),
            node.outline.as_str(),
        ],
    );
    let kind_key = match kind {
        NodeKind::Chapter => "chapter",
        NodeKind::Character => "character",
        NodeKind::SidePlot => "side_plot",
        NodeKind::Novel => unreachable!(),
    };
    let system = prompts::card_chat_system(loc, kind_key, &novel.title, &node.label, &node.outline);

    emit_chat_progress(&app, &novel_id, "thinking");
    let chat_model = task_model(&settings.chat_model, &settings.default_model);
    let (reply, _) = llm_complete(&state, &settings, &system, &content, Some(&chat_model))
        .await
        .map_err(|e| e.to_string())?;

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
    let mut models: Vec<String> = rows.iter().map(|r| r.model.clone()).collect();
    models.sort();
    models.dedup();
    Ok(TokenUsageDay { date: day, rows, models })
}

pub fn init_state() -> Result<AppState, String> {
    let db = Db::open().map_err(|e| e.to_string())?;
    crate::sample::ensure_sample(&db).map_err(|e| e.to_string())?;
    Ok(AppState { db: Arc::new(db) })
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
    fn detect_zh_over_english_ui() {
        let loc = PromptLocale::for_interaction("en", &["帮我写一个腹黑女主，每章4500字"]);
        assert!(loc.is_zh());
    }

    #[test]
    fn detect_en_input() {
        let loc = PromptLocale::for_interaction("zh-CN", &["Write a calm heroine and set 4500 words per chapter"]);
        assert_eq!(loc, PromptLocale::En);
    }
}
