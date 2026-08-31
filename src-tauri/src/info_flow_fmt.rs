//! 信息传播：写作注入时拼完整结构化正文。
use crate::models::{NodeKind, TreeNode};

const INFO_FLOW_SLOT: &str = "wv_info_flow";

pub const INFO_FLOW_INJECT_CAP: usize = 3500;

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_info_flow_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == INFO_FLOW_SLOT
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

/// 信息传播完整正文：结构化字段（优先于可能过期/截断的 extracted）。
pub fn format_info_flow_full(n: &TreeNode) -> String {
    let Some(kn) = n.knowledge.as_ref() else {
        return n.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(inf) = kn.info_flow.as_ref() {
        push_section(&mut lines, "一句话立意", &inf.premise);
        push_section(&mut lines, "信息速度", &inf.info_speed);
        push_section(&mut lines, "信息壁垒", &inf.info_barrier);
        push_section(&mut lines, "流言与真相", &inf.rumor_truth);
        push_section(&mut lines, "知识载体", &inf.knowledge_carrier);
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
        InfoFlowPayload, KnowledgeCardPayload, NodeKind, NodePosition, TreeNode,
    };

    fn kn(info_flow: Option<InfoFlowPayload>) -> TreeNode {
        TreeNode {
            id: "if".into(),
            kind: NodeKind::Knowledge,
            label: "信息传播".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: "stale".into(),
                from_canon: false,
                slot: INFO_FLOW_SLOT.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: None,
                world_race: None,
                major_faction: None,
                existence: None,
                info_flow,
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
    fn info_flow_inject_from_structured() {
        let n = kn(Some(InfoFlowPayload {
            premise: "真相滞后于流言".into(),
            info_speed: "驿马一日百里".into(),
            info_barrier: "边境查禁".into(),
            rumor_truth: "口信失真".into(),
            knowledge_carrier: "竹简与口述".into(),
        }));
        let body = format_info_flow_full(&n);
        assert!(body.contains("驿马"));
        assert!(body.contains("真相滞后"));
        assert!(body.contains("## 信息壁垒"));
        assert!(body.contains("## 流言与真相"));
        assert!(!body.contains("stale"));
    }

    #[test]
    fn rumor_truth_reads_legacy_message_truth_key() {
        let p: InfoFlowPayload = serde_json::from_value(serde_json::json!({
            "message_truth": "旧键"
        }))
        .unwrap();
        assert_eq!(p.rumor_truth, "旧键");
    }
}
