//! Chat skills via ADK-Rust (`adk-skill`).
//! Bundled: `{app resources}/skills` (crate `src-tauri/skills` in dev).
//! User extras: `~/.agents/skills` (same name wins over bundled).

use adk_rust::skill::{
    load_skill_index_with_extras, select_skill_prompt_block, select_skills, SelectionPolicy,
    SkillIndex,
};
use parking_lot::RwLock;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const RETIRED_SKILL_NAMES: &[&str] = &["nove-work"];

fn skill_listed(name: &str) -> bool {
    !RETIRED_SKILL_NAMES.contains(&name)
}

/// Truncate injected skill body (Tier-1 prompt block).
const MAX_INJECT_CHARS: usize = 8_000;
/// Auto-match threshold: description/name hits, not stray body tokens.
const AUTO_MIN_SCORE: f32 = 2.5;
const CACHE_TTL: Duration = Duration::from_secs(30);

struct Cache {
    index: Arc<SkillIndex>,
    loaded_at: Instant,
}

static CACHE: RwLock<Option<Cache>> = RwLock::new(None);
static BUNDLED_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn agents_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agents")
        .join("skills")
}

/// Crate-relative fallback (tests / `tauri dev` before resource copy).
fn crate_skills_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills")
}

/// Call once from app setup with `BaseDirectory::Resource` / `skills`.
/// Dev: prefer crate `skills/` (Tauri resource copy in `target/debug/skills` is not pruned).
/// Packaged: crate path missing → use the app resource dir.
pub fn set_bundled_dir(resolved: Option<PathBuf>) {
    let crate_dir = crate_skills_dir();
    let dir = if crate_dir.is_dir() {
        crate_dir
    } else {
        resolved.filter(|p| p.is_dir()).unwrap_or(crate_dir)
    };
    let _ = BUNDLED_DIR.set(dir);
}

pub fn bundled_skills_dir() -> PathBuf {
    BUNDLED_DIR.get().cloned().unwrap_or_else(crate_skills_dir)
}

/// User dir first so same-name skills override bundled (ADK keeps first).
fn skill_extra_dirs() -> Vec<PathBuf> {
    [agents_skills_dir(), bundled_skills_dir()]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

fn invalidate_cache() {
    *CACHE.write() = None;
}

fn load_index() -> Option<Arc<SkillIndex>> {
    {
        let guard = CACHE.read();
        if let Some(c) = guard.as_ref() {
            if c.loaded_at.elapsed() < CACHE_TTL {
                return Some(c.index.clone());
            }
        }
    }
    let extras = skill_extra_dirs();
    if extras.is_empty() {
        return None;
    }
    // Pass dirs as extras so nested `*/SKILL.md` are walked (root `.skills/` may be absent).
    let index = load_skill_index_with_extras(&extras[0], &extras).ok()?;
    let arc = Arc::new(index);
    *CACHE.write() = Some(Cache {
        index: arc.clone(),
        loaded_at: Instant::now(),
    });
    Some(arc)
}

const BODY_PREVIEW_CHARS: usize = 800;

#[derive(Debug, Clone, Serialize)]
pub struct SkillSlashHint {
    pub cmd: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillPreviewItem {
    pub name: String,
    pub description: String,
    pub path: String,
    pub trigger: bool,
    pub tags: Vec<String>,
    pub hint: Option<String>,
    pub body_preview: String,
    pub builtin: bool,
    pub slash_hints: Vec<SkillSlashHint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillsPreview {
    pub root: String,
    pub bundled: String,
    pub user: String,
    pub exists: bool,
    pub skills: Vec<SkillPreviewItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillMatchPreview {
    pub matched: bool,
    pub name: Option<String>,
    pub score: Option<f32>,
    pub via: String,
}

fn truncate_chars(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

const MCP_TOOL_PREFIXES: &[&str] = &[
    "get_", "set_", "list_", "upsert_", "search_", "import_", "archive_", "delete_", "add_",
    "update_", "link_", "unlink_", "create_", "fill_",
];

fn is_mcp_tool_name(s: &str) -> bool {
    MCP_TOOL_PREFIXES.iter().any(|p| s.starts_with(p))
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn backtick_idents(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find('`') {
        rest = &rest[i + 1..];
        let Some(j) = rest.find('`') else { break };
        let token = &rest[..j];
        rest = &rest[j + 1..];
        if is_mcp_tool_name(token) {
            out.push(token.to_string());
        }
    }
    out
}

fn line_hint(line: &str, cmd: &str) -> String {
    let stripped = line.replace('`', "");
    let left = stripped.split('→').next().unwrap_or(&stripped).trim();
    let hint = if left.is_empty() || left == cmd {
        stripped.trim()
    } else {
        left
    };
    truncate_chars(hint, 72)
}

/// SKILL.md 正文里的 MCP 工具名 → Chat `/skill cmd` 补全。
fn slash_hints_from_body(body: &str) -> Vec<SkillSlashHint> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in body.lines() {
        for cmd in backtick_idents(line) {
            if !seen.insert(cmd.clone()) {
                continue;
            }
            let hint = line_hint(line, &cmd);
            out.push(SkillSlashHint { cmd, hint });
        }
    }
    out
}

/// Settings 预览：刷新缓存并列出已发现 skills。
pub fn list_previews() -> SkillsPreview {
    invalidate_cache();
    let bundled = bundled_skills_dir();
    let user = agents_skills_dir();
    let exists = bundled.is_dir() || user.is_dir();
    let bundled_s = bundled.to_string_lossy().into_owned();
    let skills = load_index()
        .map(|idx| {
            idx.skills()
                .iter()
                .filter(|s| skill_listed(&s.name))
                .map(|s| {
                    let path = s.path.to_string_lossy().into_owned();
                    let builtin = path.starts_with(&bundled_s);
                    SkillPreviewItem {
                        name: s.name.clone(),
                        description: s.description.clone(),
                        path,
                        trigger: s.trigger,
                        tags: s.tags.clone(),
                        hint: s.hint.clone(),
                        body_preview: truncate_chars(&s.body, BODY_PREVIEW_CHARS),
                        builtin,
                        slash_hints: slash_hints_from_body(&s.body),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    SkillsPreview {
        root: user.to_string_lossy().into_owned(),
        bundled: bundled_s,
        user: user.to_string_lossy().into_owned(),
        exists,
        skills,
    }
}

/// Settings 试匹配：与 Chat 注入同一套策略（含 `@name`）。
pub fn preview_match(query: &str) -> SkillMatchPreview {
    let q = query.trim();
    if q.is_empty() {
        return SkillMatchPreview {
            matched: false,
            name: None,
            score: None,
            via: "empty".into(),
        };
    }
    let Some(index) = load_index() else {
        return SkillMatchPreview {
            matched: false,
            name: None,
            score: None,
            via: "no_dir".into(),
        };
    };
    if let Some(name) = parse_skill_name(q) {
        if skill_listed(&name) && index.find_by_name(&name).is_some() {
            return SkillMatchPreview {
                matched: true,
                name: Some(name),
                score: None,
                via: "at".into(),
            };
        }
    }
    let policy = SelectionPolicy {
        top_k: 1,
        min_score: AUTO_MIN_SCORE,
        include_tags: vec![],
        exclude_tags: vec![],
    };
    let hits = select_skills(index.as_ref(), q, &policy);
    if let Some(m) = hits.into_iter().find(|h| skill_listed(&h.skill.name)) {
        if let Some(doc) = index.find_by_id(&m.skill.id) {
            if doc.trigger {
                return SkillMatchPreview {
                    matched: false,
                    name: Some(m.skill.name),
                    score: Some(m.score),
                    via: "trigger_only".into(),
                };
            }
        }
        return SkillMatchPreview {
            matched: true,
            name: Some(m.skill.name),
            score: Some(m.score),
            via: "auto".into(),
        };
    }
    SkillMatchPreview {
        matched: false,
        name: None,
        score: None,
        via: "none".into(),
    }
}

/// Parse leading `@skill-name` or `/skill-name` (ASCII letters, digits, `-`, `_`).
fn parse_skill_name(text: &str) -> Option<String> {
    let t = text.trim_start();
    let rest = t
        .strip_prefix('@')
        .or_else(|| t.strip_prefix('/'))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// If `/known-skill …`, rewrite to `@known-skill …` for injection. Else `None`.
pub fn as_at_skill_invoke(text: &str) -> Option<String> {
    let t = text.trim_start();
    if t.starts_with('@') {
        return None;
    }
    let rest = t.strip_prefix('/')?;
    let name = parse_skill_name(t)?;
    let index = load_index()?;
    if skill_listed(&name) && index.find_by_name(&name).is_some() {
        Some(format!("@{rest}"))
    } else {
        None
    }
}


/// If a skill matches, return `(skill_name, user_text_with_skill_block)`.
///
/// - `@name …` forces that skill (also works for `trigger: true` skills).
/// - Otherwise lexical auto-select (`min_score` 2.5); skips `trigger`-only skills.
pub fn maybe_inject_skill(user_text: &str) -> Option<(String, String)> {
    let index = load_index()?;
    if index.skills().is_empty() {
        return None;
    }

    let invoke_text = as_at_skill_invoke(user_text).unwrap_or_else(|| user_text.to_string());
    if let Some(name) = parse_skill_name(&invoke_text) {
        if skill_listed(&name) {
            if let Some(doc) = index.find_by_name(&name) {
                let block = doc.engineer_prompt_block(MAX_INJECT_CHARS);
                return Some((doc.name.clone(), format!("{block}\n\n{invoke_text}")));
            }
        }
    }

    let policy = SelectionPolicy {
        top_k: 1,
        min_score: AUTO_MIN_SCORE,
        include_tags: vec![],
        exclude_tags: vec![],
    };
    let (m, block) = select_skill_prompt_block(index.as_ref(), user_text, &policy, MAX_INJECT_CHARS)?;
    if !skill_listed(&m.skill.name) {
        return None;
    }
    // trigger-only skills require @name
    if let Some(doc) = index.find_by_id(&m.skill.id) {
        if doc.trigger {
            return None;
        }
    }
    Some((m.skill.name.clone(), format!("{block}\n\n{user_text}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_skill_name_at_or_slash() {
        assert_eq!(
            parse_skill_name("@chinese-novelist 写第一章"),
            Some("chinese-novelist".into())
        );
        assert_eq!(
            parse_skill_name("/chinese-novelist 写第一章"),
            Some("chinese-novelist".into())
        );
        assert_eq!(parse_skill_name("no at"), None);
    }

    #[test]
    fn loads_skill_md_from_extra_dir() {
        let root = std::env::temp_dir().join(format!("novework-skill-test-{}", std::process::id()));
        let skill_dir = root.join("my-skill");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Write a demo chapter outline\n---\nUse short beats.\n",
        )
        .unwrap();
        let index = load_skill_index_with_extras(&root, &[root.clone()]).unwrap();
        assert!(index.find_by_name("demo-skill").is_some());
        let policy = SelectionPolicy {
            top_k: 1,
            min_score: 0.1,
            include_tags: vec![],
            exclude_tags: vec![],
        };
        let hits = adk_rust::skill::select_skills(&index, "write demo chapter outline", &policy);
        assert_eq!(hits[0].skill.name, "demo-skill");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_dir_ships_novel_skill() {
        let p = crate_skills_dir().join("novel").join("SKILL.md");
        assert!(p.is_file(), "missing {p:?}");
        assert!(
            !crate_skills_dir().join("nove-work").exists(),
            "retired skill nove-work must not ship"
        );
        let extras = [crate_skills_dir()];
        let index = load_skill_index_with_extras(&extras[0], &extras).unwrap();
        let doc = index.find_by_name("novel").expect("bundled skill");
        assert!(doc.trigger);
        for name in [
            "list_knowledge",
            "import_knowledge",
            "search_knowledge",
            "archive_knowledge",
            "delete_knowledge",
            "list_novels",
            "create_novel",
            "update_novel",
            "get_novel_info",
            "get_worldview",
            "ensure_worldview",
            "apply_worldview",
            "generate_worldview",
            "get_story_rules",
            "apply_story_rules",
            "generate_story_rules",
            "get_selected_card",
            "get_tree",
            "add_volume",
            "get_volume",
            "upsert_volume",
            "add_chapter",
            "update_chapter_outline",
            "generate_detailed_outline",
            "regenerate_detailed_outline_item",
            "delete_node",
            "get_character_card",
            "get_chapter_content",
            "get_chapter_info",
            "set_chapter_content",
            "upsert_character_card",
            "upsert_plot_card",
            "upsert_knowledge_card",
            "fill_knowledge_card",
            "list_public_knowledge_cards",
            "upsert_public_knowledge_card",
            "archive_public_knowledge_card",
            "add_public_knowledge_card",
            "link_nodes",
            "unlink_nodes",
        ] {
            assert!(
                doc.body.contains(name),
                "skills/novel missing MCP tool {name}"
            );
        }
    }

    #[test]
    fn slash_hints_from_novel_skill() {
        let body = include_str!("../skills/novel/SKILL.md");
        let hints = slash_hints_from_body(body);
        let names: Vec<&str> = hints.iter().map(|h| h.cmd.as_str()).collect();
        assert!(names.contains(&"get_selected_card"), "{names:?}");
        assert!(names.contains(&"get_novel_info"), "{names:?}");
        assert!(names.contains(&"fill_knowledge_card"), "{names:?}");
        assert!(!names.contains(&"novel_id"));
        let sel = hints.iter().find(|h| h.cmd == "get_selected_card").unwrap();
        assert!(sel.hint.contains("选中"), "{}", sel.hint);
    }
}
