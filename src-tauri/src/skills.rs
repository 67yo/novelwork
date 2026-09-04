//! Chat skills：读 `SKILL.md`（bundled + `~/.novelwork`）。
//! Bundled: `{app resources}/skills`（crate `src-tauri/skills` in dev）。
//! User extras: `~/.novelwork`（同名覆盖内置）。

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
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

#[derive(Debug, Clone)]
pub struct SkillDocument {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub trigger: bool,
    pub tags: Vec<String>,
    pub hint: Option<String>,
    pub body: String,
}

impl SkillDocument {
    pub fn engineer_prompt_block(&self, max_chars: usize) -> String {
        let mut body = self.body.clone();
        if body.chars().count() > max_chars {
            body = body.chars().take(max_chars).collect();
        }
        format!("[skill:{}]\n{}\n[/skill]", self.name, body)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillIndex {
    skills: Vec<SkillDocument>,
}

impl SkillIndex {
    pub fn skills(&self) -> &[SkillDocument] {
        &self.skills
    }

    pub fn find_by_name(&self, name: &str) -> Option<&SkillDocument> {
        self.skills.iter().find(|s| s.name == name)
    }
}

pub struct SelectionPolicy {
    pub top_k: usize,
    pub min_score: f32,
}

pub struct SkillMatch<'a> {
    pub score: f32,
    pub skill: &'a SkillDocument,
}

struct Cache {
    index: Arc<SkillIndex>,
    loaded_at: Instant,
}

static CACHE: RwLock<Option<Cache>> = RwLock::new(None);
static BUNDLED_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn user_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".novelwork")
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

/// User dir first so same-name skills override bundled.
fn skill_extra_dirs() -> Vec<PathBuf> {
    [user_skills_dir(), bundled_skills_dir()]
        .into_iter()
        .filter(|p| p.is_dir())
        .collect()
}

fn invalidate_cache() {
    *CACHE.write() = None;
}

fn discover_skill_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
                out.push(p);
            }
        }
    }
    out
}

fn parse_frontmatter(content: &str) -> (std::collections::HashMap<String, String>, String) {
    let t = content.trim_start();
    if !t.starts_with("---") {
        return (Default::default(), content.to_string());
    }
    let rest = t.trim_start_matches("---");
    let Some(end) = rest.find("\n---") else {
        return (Default::default(), content.to_string());
    };
    let yaml = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('-').trim_start().to_string();
    let mut map = std::collections::HashMap::new();
    let mut list_key: Option<String> = None;
    let mut list_vals: Vec<String> = Vec::new();
    let flush_list = |map: &mut std::collections::HashMap<String, String>,
                      list_key: &mut Option<String>,
                      list_vals: &mut Vec<String>| {
        if let Some(k) = list_key.take() {
            map.insert(k, list_vals.join(","));
            list_vals.clear();
        }
    };
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(item) = line.strip_prefix("- ") {
            if list_key.is_some() {
                list_vals.push(item.trim().trim_matches('"').to_string());
            }
            continue;
        }
        flush_list(&mut map, &mut list_key, &mut list_vals);
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim().to_string();
        let v = v.trim().trim_matches('"').to_string();
        if v.is_empty() {
            list_key = Some(k);
        } else {
            map.insert(k, v);
        }
    }
    flush_list(&mut map, &mut list_key, &mut list_vals);
    (map, body)
}

fn parse_skill_md(path: &Path, content: &str) -> Option<SkillDocument> {
    let (fm, body) = parse_frontmatter(content);
    let fallback = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("skill");
    let name = fm
        .get("name")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string();
    let description = fm.get("description").cloned().unwrap_or_default();
    let trigger = matches!(
        fm.get("trigger").map(|s| s.as_str()),
        Some("true") | Some("True") | Some("yes") | Some("1")
    );
    let tags = fm
        .get("tags")
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let hint = fm
        .get("hint")
        .cloned()
        .filter(|s| !s.is_empty());
    Some(SkillDocument {
        name,
        description,
        path: path.to_path_buf(),
        trigger,
        tags,
        hint,
        body,
    })
}

pub fn load_skill_index_with_extras(
    _root: &Path,
    extra_dirs: &[PathBuf],
) -> Result<SkillIndex, String> {
    let mut skills = Vec::new();
    let mut seen = HashSet::new();
    for dir in extra_dirs {
        if !dir.is_dir() {
            continue;
        }
        for path in discover_skill_files(dir) {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(doc) = parse_skill_md(&path, &content) else {
                continue;
            };
            if !seen.insert(doc.name.clone()) {
                continue;
            }
            skills.push(doc);
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    Ok(SkillIndex { skills })
}

fn tokenize(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| t.chars().count() >= 2)
        .collect()
}

pub fn select_skills<'a>(
    index: &'a SkillIndex,
    query: &str,
    policy: &SelectionPolicy,
) -> Vec<SkillMatch<'a>> {
    if policy.top_k == 0 {
        return Vec::new();
    }
    let q = tokenize(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<SkillMatch<'a>> = index
        .skills()
        .iter()
        .filter_map(|skill| {
            let name = tokenize(&skill.name);
            let desc = tokenize(&skill.description);
            let body = tokenize(&skill.body);
            let tags: HashSet<String> = skill.tags.iter().flat_map(|t| tokenize(t)).collect();
            let mut score = 0.0;
            for token in &q {
                if name.contains(token) {
                    score += 4.0;
                }
                if desc.contains(token) {
                    score += 2.5;
                }
                if tags.contains(token) {
                    score += 2.0;
                }
                if body.contains(token) {
                    score += 1.0;
                }
            }
            (score >= policy.min_score).then_some(SkillMatch { score, skill })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.skill.name.cmp(&b.skill.name))
    });
    scored.truncate(policy.top_k);
    scored
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
    let user = user_skills_dir();
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

/// Settings 试匹配：`/name` 或 `@name` 即命中；纯文本只提示需显式调用（Chat 不自动注入）。
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
    };
    let hits = select_skills(index.as_ref(), q, &policy);
    if let Some(m) = hits.into_iter().find(|h| skill_listed(&h.skill.name)) {
        // Chat 只在 /name 或 @name 时注入；试匹配仍提示命中了哪个 skill。
        return SkillMatchPreview {
            matched: false,
            name: Some(m.skill.name.clone()),
            score: Some(m.score),
            via: "trigger_only".into(),
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


/// If the user explicitly invoked a skill (`/name` or `@name`), return
/// `(skill_name, user_text_with_skill_block)`. No lexical auto-inject.
pub fn maybe_inject_skill(user_text: &str) -> Option<(String, String)> {
    let index = load_index()?;
    if index.skills().is_empty() {
        return None;
    }

    let invoke_text = as_at_skill_invoke(user_text).unwrap_or_else(|| user_text.to_string());
    let name = parse_skill_name(&invoke_text)?;
    if !skill_listed(&name) {
        return None;
    }
    let doc = index.find_by_name(&name)?;
    let block = doc.engineer_prompt_block(MAX_INJECT_CHARS);
    Some((doc.name.clone(), format!("{block}\n\n{invoke_text}")))
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
    fn injects_only_on_explicit_slash_or_at() {
        assert!(maybe_inject_skill("生成第 19 章正文").is_none());
        assert!(maybe_inject_skill("写小说第一章").is_none());
        let hit = maybe_inject_skill("/novel 生成第 1 章").expect("bundled /novel");
        assert_eq!(hit.0, "novel");
        assert!(hit.1.contains("get_chapter_write_context"));
        let at = maybe_inject_skill("@novel get_selected_card").expect("@novel");
        assert_eq!(at.0, "novel");
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
        };
        let hits = select_skills(&index, "write demo chapter outline", &policy);
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
            "get_chapter_write_context",
            "set_chapter_content",
            "get_chapter_shots",
            "set_chapter_shots",
            "split_chapter_shots",
            "generate_shot_comfy_prompts",
            "submit_chapter_shots_comfyui",
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
