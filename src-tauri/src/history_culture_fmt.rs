//! 历史文化 / 宗教 / 重大事件：写作注入时拼完整正文（含子卡）。
use crate::models::{HistoryCulturePayload, MajorEvent, NodeKind, NovelTree, TreeNode, WorldReligion};

const HISTORY_CULTURE_SLOT: &str = "wv_history_culture";
const RELIGION_SLOT: &str = "wv_religion";
const MAJOR_EVENT_SLOT: &str = "wv_major_event";

pub const HISTORY_CULTURE_INJECT_CAP: usize = 3500;

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_history_culture_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == HISTORY_CULTURE_SLOT
}

fn linked_by_slot<'a>(tree: &'a NovelTree, host_id: &str, slot: &str) -> Vec<&'a TreeNode> {
    let mut ids = std::collections::HashSet::new();
    if let Some(host) = tree.nodes.iter().find(|n| n.id == host_id) {
        for id in &host.linked_knowledge_ids {
            ids.insert(id.clone());
        }
    }
    for e in &tree.edges {
        if e.kind != "knowledge" {
            continue;
        }
        let other = if e.source == host_id {
            Some(e.target.as_str())
        } else if e.target == host_id {
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
            if matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == slot {
                Some(n)
            } else {
                None
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.position
            .x
            .partial_cmp(&b.position.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.position
                    .y
                    .partial_cmp(&b.position.y)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.id.cmp(&b.id))
    });
    out
}

fn religion_has_content(r: &WorldReligion) -> bool {
    !r.name.trim().is_empty()
        || !r.core_belief.trim().is_empty()
        || !r.followers_scope.trim().is_empty()
}

fn event_has_content(e: &MajorEvent) -> bool {
    !e.title.trim().is_empty()
        || !e.event.trim().is_empty()
        || !e.long_term_impact.trim().is_empty()
}

fn push_religion(lines: &mut Vec<String>, r: &WorldReligion, idx: usize) {
    let title = if r.name.trim().is_empty() {
        idx.to_string()
    } else {
        r.name.trim().to_string()
    };
    lines.push(format!("### {title}"));
    if !r.core_belief.trim().is_empty() {
        lines.push(format!("- 核心信念：{}", r.core_belief.trim()));
    }
    if !r.followers_scope.trim().is_empty() {
        lines.push(format!("- 追随者范围：{}", r.followers_scope.trim()));
    }
    lines.push(String::new());
}

fn push_event(lines: &mut Vec<String>, e: &MajorEvent, idx: usize) {
    let title = if e.title.trim().is_empty() {
        idx.to_string()
    } else {
        e.title.trim().to_string()
    };
    lines.push(format!("### {title}"));
    if !e.event.trim().is_empty() {
        lines.push(format!("- 事件：{}", e.event.trim()));
    }
    if !e.long_term_impact.trim().is_empty() {
        lines.push(format!("- 长期影响：{}", e.long_term_impact.trim()));
    }
    lines.push(String::new());
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

pub fn format_history_culture_full(tree: &NovelTree, host: &TreeNode) -> String {
    let Some(kn) = host.knowledge.as_ref() else {
        return host.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(hc) = kn.history_culture.as_ref() {
        push_section(&mut lines, "一句话立意", &hc.premise);
        push_section(&mut lines, "风俗习惯", &hc.customs);
        push_section(&mut lines, "经济体系", &hc.economy);
        let mut religions: Vec<WorldReligion> = linked_by_slot(tree, &host.id, RELIGION_SLOT)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.world_religion.clone())
            .filter(|r| religion_has_content(r))
            .collect();
        if religions.is_empty() {
            religions = hc
                .religions
                .iter()
                .filter(|r| religion_has_content(r))
                .cloned()
                .collect();
        }
        if !religions.is_empty() {
            lines.push("## 宗教".into());
            for (i, r) in religions.iter().enumerate() {
                push_religion(&mut lines, r, i + 1);
            }
        }
        push_section(&mut lines, "日常切片", &hc.daily_slices);
        let mut events: Vec<MajorEvent> = linked_by_slot(tree, &host.id, MAJOR_EVENT_SLOT)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.major_event.clone())
            .filter(|e| event_has_content(e))
            .collect();
        if events.is_empty() {
            events = hc
                .major_events
                .iter()
                .filter(|e| event_has_content(e))
                .cloned()
                .collect();
        }
        if !events.is_empty() {
            lines.push("## 重大事件".into());
            for (i, e) in events.iter().enumerate() {
                push_event(&mut lines, e, i + 1);
            }
        }
    }
    let out = lines.join("\n").trim().to_string();
    if !out.is_empty() {
        return out;
    }
    let feat = kn.extracted.trim();
    if !feat.is_empty() {
        return feat.to_string();
    }
    host.outline.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        HistoryCulturePayload, KnowledgeCardPayload, NodeKind, NodePosition, TreeEdge, TreeNode,
    };

    fn kn(
        id: &str,
        slot: &str,
        history_culture: Option<HistoryCulturePayload>,
        world_religion: Option<WorldReligion>,
        major_event: Option<MajorEvent>,
    ) -> TreeNode {
        TreeNode {
            id: id.into(),
            kind: NodeKind::Knowledge,
            label: id.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: "stale".into(),
                from_canon: false,
                slot: slot.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: None,
                world_race: None,
                major_faction: None,
                existence: None,
                info_flow: None,
                history_culture,
                world_religion,
                major_event,
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
    fn history_culture_inject_includes_linked_children() {
        let mut host = kn(
            "hc",
            HISTORY_CULTURE_SLOT,
            Some(HistoryCulturePayload {
                premise: "礼法与商路".into(),
                customs: "祭河".into(),
                economy: "盐铁".into(),
                religions: vec![],
                daily_slices: "早市".into(),
                major_events: vec![],
            }),
            None,
            None,
        );
        host.linked_knowledge_ids = vec!["rel1".into(), "ev1".into()];
        let rel = kn(
            "rel1",
            RELIGION_SLOT,
            None,
            Some(WorldReligion {
                name: "河神教".into(),
                core_belief: "顺流则昌".into(),
                followers_scope: "渔民".into(),
            }),
            None,
        );
        let ev = kn(
            "ev1",
            MAJOR_EVENT_SLOT,
            None,
            None,
            Some(MajorEvent {
                title: "盐门之变".into(),
                event: "夺权".into(),
                long_term_impact: "集权".into(),
            }),
        );
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![host, rel, ev],
            edges: vec![],
        };
        let body = format_history_culture_full(&tree, &tree.nodes[0]);
        assert!(body.contains("礼法与商路"));
        assert!(body.contains("### 河神教"));
        assert!(body.contains("### 盐门之变"));
        assert!(!body.contains("stale"));
    }
}
