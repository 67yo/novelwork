//! 社会权力 / 种族 / 主要势力：写作注入时拼完整正文（含子卡）。
use crate::models::{MajorFaction, NodeKind, NovelTree, SocialPowerPayload, TreeNode, WorldRace};

const RACE_SLOT: &str = "wv_race";
const FACTION_SLOT: &str = "wv_faction";
const SOCIAL_POWER_SLOT: &str = "wv_social_power";

pub const SOCIAL_POWER_INJECT_CAP: usize = 3500;

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_social_power_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == SOCIAL_POWER_SLOT
}

fn linked_child_nodes<'a>(
    tree: &'a NovelTree,
    host_id: &str,
    slot: &str,
) -> Vec<&'a TreeNode> {
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

pub fn linked_race_nodes<'a>(tree: &'a NovelTree, host_id: &str) -> Vec<&'a TreeNode> {
    linked_child_nodes(tree, host_id, RACE_SLOT)
}

pub fn linked_faction_nodes<'a>(tree: &'a NovelTree, host_id: &str) -> Vec<&'a TreeNode> {
    linked_child_nodes(tree, host_id, FACTION_SLOT)
}

fn race_has_content(r: &WorldRace) -> bool {
    !r.name.trim().is_empty()
        || !r.features.trim().is_empty()
        || !r.population.trim().is_empty()
        || !r.social_status.trim().is_empty()
}

fn faction_has_content(f: &MajorFaction) -> bool {
    !f.name.trim().is_empty()
        || !f.faction_type.trim().is_empty()
        || !f.goal.trim().is_empty()
        || !f.means.trim().is_empty()
        || !f.power_base.trim().is_empty()
}

fn push_race(lines: &mut Vec<String>, r: &WorldRace) {
    lines.push(format!("### {}", r.name.trim()));
    if !r.features.trim().is_empty() {
        lines.push(format!("- 特征：{}", r.features.trim()));
    }
    if !r.population.trim().is_empty() {
        lines.push(format!("- 人口：{}", r.population.trim()));
    }
    if !r.social_status.trim().is_empty() {
        lines.push(format!("- 社会地位：{}", r.social_status.trim()));
    }
    lines.push(String::new());
}

fn push_faction(lines: &mut Vec<String>, f: &MajorFaction) {
    let title = if f.name.trim().is_empty() {
        f.faction_type.trim()
    } else {
        f.name.trim()
    };
    lines.push(format!("### {title}"));
    if !f.faction_type.trim().is_empty() {
        lines.push(format!("- 类型：{}", f.faction_type.trim()));
    }
    if !f.goal.trim().is_empty() {
        lines.push(format!("- 目标：{}", f.goal.trim()));
    }
    if !f.means.trim().is_empty() {
        lines.push(format!("- 手段：{}", f.means.trim()));
    }
    if !f.power_base.trim().is_empty() {
        lines.push(format!("- 权力基础：{}", f.power_base.trim()));
    }
    lines.push(String::new());
}

pub fn format_social_power_full(tree: &NovelTree, host: &TreeNode) -> String {
    let Some(kn) = host.knowledge.as_ref() else {
        return host.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(sp) = kn.social_power.as_ref() {
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
        let mut races: Vec<WorldRace> = linked_race_nodes(tree, &host.id)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.world_race.clone())
            .filter(|r| race_has_content(r))
            .collect();
        if races.is_empty() {
            races = sp
                .races
                .iter()
                .filter(|r| race_has_content(r))
                .cloned()
                .collect();
        }
        if !races.is_empty() {
            lines.push("## 种族".into());
            for r in &races {
                push_race(&mut lines, r);
            }
        }
        let mut factions: Vec<MajorFaction> = linked_faction_nodes(tree, &host.id)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.major_faction.clone())
            .filter(|f| faction_has_content(f))
            .collect();
        if factions.is_empty() {
            factions = sp
                .factions
                .iter()
                .filter(|f| faction_has_content(f))
                .cloned()
                .collect();
        }
        if !factions.is_empty() {
            lines.push("## 主要势力".into());
            for f in &factions {
                push_faction(&mut lines, f);
            }
        }
        push_section(&mut lines, "阶层结构", &sp.class_structure);
        push_section(&mut lines, "政治体制", &sp.political_system);
        push_section(&mut lines, "权力可见性", &sp.power_visibility);
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
        KnowledgeCardPayload, NodeKind, NodePosition, SocialPowerPayload, TreeEdge, TreeNode,
    };

    #[test]
    fn social_power_inject_includes_linked_cards() {
        let mut host = TreeNode {
            id: "sp".into(),
            kind: NodeKind::Knowledge,
            label: "社会权力".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: "stale".into(),
                from_canon: false,
                slot: SOCIAL_POWER_SLOT.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: Some(SocialPowerPayload {
                    premise: "权力即契约".into(),
                    races: vec![],
                    factions: vec![],
                    class_structure: "三层".into(),
                    political_system: "寡头".into(),
                    power_visibility: "隐秘".into(),
                }),
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
            linked_knowledge_ids: vec!["r1".into(), "f1".into()],
            position: NodePosition { x: 0.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        };
        let race = TreeNode {
            id: "r1".into(),
            kind: NodeKind::Knowledge,
            label: "角民".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: String::new(),
                from_canon: false,
                slot: RACE_SLOT.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: None,
                world_race: Some(WorldRace {
                    name: "角民".into(),
                    features: "头生角".into(),
                    population: "80%".into(),
                    social_status: "底层劳工".into(),
                }),
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
        };
        let faction = TreeNode {
            id: "f1".into(),
            kind: NodeKind::Knowledge,
            label: "同盟".into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: Some(KnowledgeCardPayload {
                book_ids: vec![],
                extract_prompt: String::new(),
                extracted: String::new(),
                from_canon: false,
                slot: FACTION_SLOT.into(),
                core_laws: None,
                spatiotemporal: None,
                world_axiom: None,
                key_location: None,
                social_power: None,
                world_race: None,
                major_faction: Some(MajorFaction {
                    name: "北境同盟".into(),
                    faction_type: "政治军事联盟".into(),
                    goal: "统一关税".into(),
                    means: "联姻与雇佣军".into(),
                    power_base: "雾港".into(),
                }),
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
        };
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![host, race, faction],
            edges: vec![
                TreeEdge {
                    id: "e1".into(),
                    source: "sp".into(),
                    target: "r1".into(),
                    kind: "knowledge".into(),
                    source_handle: Some("top".into()),
                    target_handle: Some("bottom".into()),
                    label: String::new(),
                },
                TreeEdge {
                    id: "e2".into(),
                    source: "sp".into(),
                    target: "f1".into(),
                    kind: "knowledge".into(),
                    source_handle: Some("top".into()),
                    target_handle: Some("bottom".into()),
                    label: String::new(),
                },
            ],
        };
        let body = format_social_power_full(&tree, &tree.nodes[0]);
        assert!(body.contains("权力即契约"));
        assert!(body.contains("### 角民"));
        assert!(body.contains("头生角"));
        assert!(body.contains("### 北境同盟"));
        assert!(body.contains("政治军事联盟"));
        assert!(!body.contains("stale"));
    }
}
