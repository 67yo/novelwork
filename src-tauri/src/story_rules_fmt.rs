//! 故事规则四卡：写作注入拼完整结构化正文。
use crate::models::{
    ConstraintRedlinesPayload, FulfillmentSystemPayload, NodeKind, NovelTree, StoryEnginePayload,
    SurfaceSettingPayload, TreeNode,
};

const STORY_RULES_SLOT: &str = "story_rules";

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_story_rules_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == STORY_RULES_SLOT
}

pub fn is_story_rules_fan_slot(slot: &str) -> bool {
    matches!(
        slot,
        "sr_surface_setting"
            | "sr_story_engine"
            | "sr_fulfillment_system"
            | "sr_constraint_redlines"
    )
}

pub fn is_story_rules_fan_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && is_story_rules_fan_slot(knowledge_slot(n))
}

fn push_section(lines: &mut Vec<String>, title: &str, body: &str) {
    let t = body.trim();
    if t.is_empty() {
        return;
    }
    lines.push(format!("## {}", title));
    lines.push(t.to_string());
    lines.push(String::new());
}

fn push_list(lines: &mut Vec<String>, title: &str, items: &[String]) {
    let list: Vec<&str> = items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if list.is_empty() {
        return;
    }
    lines.push(format!("## {}", title));
    for (i, x) in list.iter().enumerate() {
        lines.push(format!("{}. {}", i + 1, x));
    }
    lines.push(String::new());
}

pub fn format_surface_setting(d: &SurfaceSettingPayload) -> String {
    let mut lines: Vec<String> = Vec::new();
    push_section(&mut lines, "一句话立意", &d.premise);
    push_section(&mut lines, "核心冲突", &d.core_conflict);
    push_section(&mut lines, "读者承诺", &d.reader_promise);
    push_section(&mut lines, "目标读者", &d.target_audience);
    push_section(&mut lines, "基调参照", &d.tone_reference);
    push_section(&mut lines, "商业标签", &d.commercial_tags);
    push_section(&mut lines, "扩展立意", &d.extended_premise);
    lines.join("\n").trim().to_string()
}

pub fn format_story_engine(d: &StoryEnginePayload) -> String {
    let mut lines: Vec<String> = Vec::new();
    push_section(&mut lines, "一句话立意", &d.premise);
    if !d.bright_line.trim().is_empty() || !d.dark_line.trim().is_empty() {
        lines.push("## 明暗双线".into());
        if !d.bright_line.trim().is_empty() {
            lines.push("### 明线".into());
            lines.push(d.bright_line.trim().to_string());
            lines.push(String::new());
        }
        if !d.dark_line.trim().is_empty() {
            lines.push("### 暗线".into());
            lines.push(d.dark_line.trim().to_string());
            lines.push(String::new());
        }
    }
    push_section(&mut lines, "悬念设定", &d.suspense_setup);
    push_section(&mut lines, "冲突引擎", &d.conflict_engine);
    if !d.external_conflict.trim().is_empty()
        || !d.internal_conflict.trim().is_empty()
        || !d.relational_conflict.trim().is_empty()
    {
        lines.push("## 冲突层级".into());
        if !d.external_conflict.trim().is_empty() {
            lines.push("### 外部".into());
            lines.push(d.external_conflict.trim().to_string());
            lines.push(String::new());
        }
        if !d.internal_conflict.trim().is_empty() {
            lines.push("### 内部".into());
            lines.push(d.internal_conflict.trim().to_string());
            lines.push(String::new());
        }
        if !d.relational_conflict.trim().is_empty() {
            lines.push("### 关系".into());
            lines.push(d.relational_conflict.trim().to_string());
            lines.push(String::new());
        }
    }
    push_section(&mut lines, "推进循环", &d.progression_cycle);
    push_section(&mut lines, "主角困局", &d.protagonist_dilemma);
    lines.join("\n").trim().to_string()
}

pub fn format_fulfillment_system(d: &FulfillmentSystemPayload) -> String {
    let mut lines: Vec<String> = Vec::new();
    push_section(&mut lines, "一句话立意", &d.premise);
    push_section(&mut lines, "成长路径", &d.growth_path);
    push_section(&mut lines, "结局质感", &d.ending_texture);
    push_list(&mut lines, "兑现语法", &d.payoff_syntax);
    push_section(&mut lines, "情绪节奏", &d.emotional_rhythm);
    push_list(&mut lines, "张力原型", &d.tension_archetypes);
    lines.join("\n").trim().to_string()
}

pub fn format_constraint_redlines(d: &ConstraintRedlinesPayload) -> String {
    let mut lines: Vec<String> = Vec::new();
    push_section(&mut lines, "一句话立意", &d.premise);
    push_list(&mut lines, "约束红线", &d.redlines);
    lines.join("\n").trim().to_string()
}

pub fn format_story_rules_fan_full(_tree: &NovelTree, n: &TreeNode) -> String {
    let slot = knowledge_slot(n);
    let k = n.knowledge.as_ref();
    match slot {
        "sr_surface_setting" => k
            .and_then(|x| x.surface_setting.as_ref())
            .map(format_surface_setting)
            .unwrap_or_default(),
        "sr_story_engine" => k
            .and_then(|x| x.story_engine.as_ref())
            .map(format_story_engine)
            .unwrap_or_default(),
        "sr_fulfillment_system" => k
            .and_then(|x| x.fulfillment_system.as_ref())
            .map(format_fulfillment_system)
            .unwrap_or_default(),
        "sr_constraint_redlines" => k
            .and_then(|x| x.constraint_redlines.as_ref())
            .map(format_constraint_redlines)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn linked_story_rules_blocks<'a>(tree: &'a NovelTree, rules_id: &str) -> Vec<&'a TreeNode> {
    let mut ids = std::collections::HashSet::new();
    if let Some(rules) = tree.nodes.iter().find(|n| n.id == rules_id) {
        for id in &rules.linked_knowledge_ids {
            ids.insert(id.clone());
        }
    }
    for e in &tree.edges {
        if e.kind != "knowledge" {
            continue;
        }
        let other = if e.source == rules_id {
            Some(e.target.as_str())
        } else if e.target == rules_id {
            Some(e.source.as_str())
        } else {
            None
        };
        if let Some(oid) = other {
            ids.insert(oid.to_string());
        }
    }
    let mut out: Vec<&TreeNode> = ids
        .iter()
        .filter_map(|id| {
            let n = tree.nodes.iter().find(|n| n.id == *id)?;
            if is_story_rules_fan_card(n) {
                Some(n)
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "sr_surface_setting" => 0,
            "sr_story_engine" => 1,
            "sr_fulfillment_system" => 2,
            "sr_constraint_redlines" => 3,
            _ => 9,
        };
        rank(knowledge_slot(a))
            .cmp(&rank(knowledge_slot(b)))
            .then(
                a.position
                    .x
                    .partial_cmp(&b.position.x)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.id.cmp(&b.id))
    });
    out
}

pub fn format_story_rules_full(tree: &NovelTree, rules: &TreeNode) -> String {
    let mut parts: Vec<String> = Vec::new();
    for child in linked_story_rules_blocks(tree, &rules.id) {
        let body = format_story_rules_fan_full(tree, child);
        if body.trim().is_empty() {
            continue;
        }
        parts.push(format!("# {}", child.label.trim()));
        parts.push(body);
    }
    let joined = parts.join("\n\n").trim().to_string();
    if !joined.is_empty() {
        return joined;
    }
    rules
        .knowledge
        .as_ref()
        .map(|k| k.extracted.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::models::FulfillmentSystemPayload;

    #[test]
    fn tension_archetypes_reads_legacy_circles_key() {
        let p: FulfillmentSystemPayload = serde_json::from_value(serde_json::json!({
            "tension_circles": ["旧键"]
        }))
        .unwrap();
        assert_eq!(p.tension_archetypes, vec!["旧键"]);
    }
}
