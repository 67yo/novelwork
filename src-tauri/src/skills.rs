//! Chat skills via ADK-Rust (`adk-skill`), loaded from `~/.agents/skills`.

use adk_rust::skill::{
    load_skill_index_with_extras, select_skill_prompt_block, select_skills, SelectionPolicy,
    SkillIndex,
};
use parking_lot::RwLock;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

pub fn agents_skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agents")
        .join("skills")
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
    let dir = agents_skills_dir();
    if !dir.is_dir() {
        return None;
    }
    // Pass dir as extra so nested `*/SKILL.md` are walked (root `.skills/` may be absent).
    let index = load_skill_index_with_extras(&dir, std::slice::from_ref(&dir)).ok()?;
    let arc = Arc::new(index);
    *CACHE.write() = Some(Cache {
        index: arc.clone(),
        loaded_at: Instant::now(),
    });
    Some(arc)
}

const BODY_PREVIEW_CHARS: usize = 800;

#[derive(Debug, Clone, Serialize)]
pub struct SkillPreviewItem {
    pub name: String,
    pub description: String,
    pub path: String,
    pub trigger: bool,
    pub tags: Vec<String>,
    pub hint: Option<String>,
    pub body_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillsPreview {
    pub root: String,
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

/// Settings 预览：刷新缓存并列出已发现 skills。
pub fn list_previews() -> SkillsPreview {
    invalidate_cache();
    let root = agents_skills_dir();
    let exists = root.is_dir();
    let skills = load_index()
        .map(|idx| {
            idx.skills()
                .iter()
                .map(|s| SkillPreviewItem {
                    name: s.name.clone(),
                    description: s.description.clone(),
                    path: s.path.to_string_lossy().into_owned(),
                    trigger: s.trigger,
                    tags: s.tags.clone(),
                    hint: s.hint.clone(),
                    body_preview: truncate_chars(&s.body, BODY_PREVIEW_CHARS),
                })
                .collect()
        })
        .unwrap_or_default();
    SkillsPreview {
        root: root.to_string_lossy().into_owned(),
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
        if index.find_by_name(&name).is_some() {
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
    if let Some(m) = hits.into_iter().next() {
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
    if index.find_by_name(&name).is_some() {
        Some(format!("@{rest}"))
    } else {
        None
    }
}

/// True when `/name` names a loaded skill (used to skip unknown-slash help).
pub fn is_slash_skill_invoke(text: &str) -> bool {
    as_at_skill_invoke(text).is_some()
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
        if let Some(doc) = index.find_by_name(&name) {
            let block = doc.engineer_prompt_block(MAX_INJECT_CHARS);
            return Some((doc.name.clone(), format!("{block}\n\n{invoke_text}")));
        }
    }

    let policy = SelectionPolicy {
        top_k: 1,
        min_score: AUTO_MIN_SCORE,
        include_tags: vec![],
        exclude_tags: vec![],
    };
    let (m, block) = select_skill_prompt_block(index.as_ref(), user_text, &policy, MAX_INJECT_CHARS)?;
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
}
