//! 存在基础：写作注入时拼完整结构化正文。
use crate::models::{NodeKind, TreeNode};

const EXISTENCE_SLOT: &str = "wv_existence";

pub const EXISTENCE_INJECT_CAP: usize = 3500;

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_existence_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == EXISTENCE_SLOT
}

fn push_section(lines: &mut Vec<String>, title: &str, body: &str) {
    let b = body.trim();
    if b.is_empty() {
        return;
    }
    lines.push(format!("## {title}"));
    lines.push(b.to_string());
    lines.push(String::new());
}

/// 存在基础完整正文：结构化字段（优先于可能过期/截断的 extracted）。
pub fn format_existence_full(n: &TreeNode) -> String {
    let Some(kn) = n.knowledge.as_ref() else {
        return n.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(ex) = kn.existence.as_ref() {
        push_section(&mut lines, "一句话立意", &ex.premise);
        push_section(&mut lines, "死亡", &ex.death);
        push_section(&mut lines, "历法", &ex.calendar);
        push_section(&mut lines, "寿命", &ex.lifespan);
        push_section(&mut lines, "疫病与繁衍", &ex.disease_reproduction);
    }
    let out = lines.join("\n").trim().to_string();
    if !out.is_empty() {
        return out;
    }
    let feat = kn.extracted.trim();
    if !feat.is_empty() {
        return feat.to_string();
    }
    n.outline.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ExistencePayload, KnowledgeCardPayload, NodeKind, NodePosition, TreeNode,
    };

    fn kn(existence: Option<ExistencePayload>) -> TreeNode {
        TreeNode {
            id: "ex".into(),
            kind: NodeKind::Knowledge,
            label: "存在基础".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: "stale".into(),
                from_canon: false,
                slot: EXISTENCE_SLOT.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: None,
                world_race: None,
                major_faction: None,
                existence,
                info_flow: None,
                history_culture: None,
                world_religion: None,
                major_event: None,
                surface_setting: None,
                story_engine: None,
                fulfillment_system: None,
                constraint_redlines: None,
            }),
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: vec![],
            linked_knowledge_ids: vec![],
            position: NodePosition { x: 0.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        }
    }

    #[test]
    fn existence_inject_from_structured() {
        let n = kn(Some(ExistencePayload {
            premise: "生死有账".into(),
            death: "灵魂不散".into(),
            calendar: "双月历".into(),
            lifespan: "凡人八十".into(),
            disease_reproduction: "瘟疫周期十年".into(),
        }));
        let body = format_existence_full(&n);
        assert!(body.contains("生死有账"));
        assert!(body.contains("## 死亡"));
        assert!(body.contains("双月历"));
        assert!(!body.contains("stale"));
    }
}
