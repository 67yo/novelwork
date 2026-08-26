//! 时空地理 / 关键地点：写作注入时拼完整正文（含挂在时空地理上的地点子卡）。
use crate::models::{KeyLocation, NodeKind, NovelTree, TreeNode};

const LOCATION_SLOT: &str = "wv_location";
const SPATIOTEMPORAL_SLOT: &str = "wv_spatiotemporal";

/// 时空地理结构化卡单卡注入上限（须能装下完整地点列表）。
pub const SPATIOTEMPORAL_INJECT_CAP: usize = 3500;

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_spatiotemporal_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == SPATIOTEMPORAL_SLOT
}

pub fn linked_location_nodes<'a>(tree: &'a NovelTree, st_id: &str) -> Vec<&'a TreeNode> {
    let mut ids = std::collections::HashSet::new();
    if let Some(st) = tree.nodes.iter().find(|n| n.id == st_id) {
        for id in &st.linked_knowledge_ids {
            ids.insert(id.clone());
        }
    }
    for e in &tree.edges {
        if e.kind != "knowledge" {
            continue;
        }
        let other = if e.source == st_id {
            Some(e.target.as_str())
        } else if e.target == st_id {
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
            if matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == LOCATION_SLOT {
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

fn location_has_content(l: &KeyLocation) -> bool {
    !l.name.trim().is_empty()
        || !l.features.trim().is_empty()
        || !l.terrain.trim().is_empty()
        || !l.faction.trim().is_empty()
}

fn push_location(lines: &mut Vec<String>, l: &KeyLocation) {
    lines.push(format!("### {}", l.name.trim()));
    if !l.features.trim().is_empty() {
        lines.push(format!("- 特征：{}", l.features.trim()));
    }
    if !l.terrain.trim().is_empty() {
        lines.push(format!("- 地貌：{}", l.terrain.trim()));
    }
    if !l.faction.trim().is_empty() {
        lines.push(format!("- 控制势力：{}", l.faction.trim()));
    }
    lines.push(String::new());
}

/// 时空地理完整正文：结构化字段 + 地点子卡（优先于可能过期/截断的 extracted）。
pub fn format_spatiotemporal_full(tree: &NovelTree, st: &TreeNode) -> String {
    let Some(kn) = st.knowledge.as_ref() else {
        return st.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(sp) = kn.spatiotemporal.as_ref() {
        let push_section = |lines: &mut Vec<String>, title: &str, body: &str| {
            let t = body.trim();
            if t.is_empty() {
                return;
            }
            lines.push(format!("## {title}"));
            lines.push(t.to_string());
            lines.push(String::new());
        };
        push_section(&mut lines, "一句话立意", &sp.premise);
        push_section(&mut lines, "时代背景", &sp.era);
        push_section(&mut lines, "生态", &sp.ecology);
        push_section(&mut lines, "世界格局", &sp.world_pattern);
        let mut locs: Vec<KeyLocation> = linked_location_nodes(tree, &st.id)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.key_location.clone())
            .filter(|l| location_has_content(l))
            .collect();
        if locs.is_empty() {
            locs = sp
                .locations
                .iter()
                .filter(|l| location_has_content(l))
                .cloned()
                .collect();
        }
        if !locs.is_empty() {
            lines.push("## 关键地点".into());
            for l in &locs {
                push_location(&mut lines, l);
            }
        }
        push_section(&mut lines, "环境质感", &sp.atmosphere);
    }
    let out = lines.join("\n").trim().to_string();
    if !out.is_empty() {
        return out;
    }
    let feat = kn.extracted.trim();
    if !feat.is_empty() {
        return feat.to_string();
    }
    st.outline.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        KnowledgeCardPayload, NodeKind, NodePosition, SpatiotemporalPayload, TreeEdge, TreeNode,
    };

    fn kn(
        id: &str,
        slot: &str,
        spatiotemporal: Option<SpatiotemporalPayload>,
        key_location: Option<KeyLocation>,
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
                spatiotemporal,
                world_axiom: None,
                key_location,
                social_power: None,
                world_race: None,
                major_faction: None,
                existence: None,
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
    fn spatiotemporal_inject_includes_linked_locations() {
        let mut st = kn(
            "st",
            SPATIOTEMPORAL_SLOT,
            Some(SpatiotemporalPayload {
                premise: "海雾世界".into(),
                era: "蒸汽".into(),
                ecology: String::new(),
                world_pattern: String::new(),
                locations: vec![],
                atmosphere: "咸湿".into(),
            }),
            None,
        );
        st.linked_knowledge_ids = vec!["loc1".into()];
        let loc = kn(
            "loc1",
            LOCATION_SLOT,
            None,
            Some(KeyLocation {
                name: "雾港".into(),
                features: "贸易".into(),
                terrain: "海湾".into(),
                faction: "商会".into(),
            }),
        );
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![st, loc],
            edges: vec![TreeEdge {
                id: "e".into(),
                source: "st".into(),
                target: "loc1".into(),
                kind: "knowledge".into(),
                source_handle: Some("top".into()),
                target_handle: Some("bottom".into()),
                label: String::new(),
            }],
        };
        let body = format_spatiotemporal_full(&tree, &tree.nodes[0]);
        assert!(body.contains("海雾世界"));
        assert!(body.contains("### 雾港"));
        assert!(body.contains("贸易"));
        assert!(!body.contains("stale"));
    }
}
