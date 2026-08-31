//! 核心法则 / 世界公理：写作注入时拼完整正文（含挂在核心法则上的公理子卡）。
use crate::models::{NodeKind, NovelTree, TreeNode, WorldAxiom};

const AXIOM_SLOT: &str = "wv_axiom";
const CORE_LAWS_SLOT: &str = "wv_core_laws";

/// 世界观结构化卡单卡注入上限（须能装下完整公理列表）。
pub const CORE_LAWS_INJECT_CAP: usize = 3500;

/// 有核心法则时，总预算在 base 上追加「超出普通单卡」的差额，避免挤掉其它卡。
pub fn knowledge_inject_total(base: usize) -> usize {
    base + CORE_LAWS_INJECT_CAP.saturating_sub(crate::kb_context::KNOWLEDGE_CARD_EXTRACT_CAP)
}

pub fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

pub fn is_core_laws_card(n: &TreeNode) -> bool {
    matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == CORE_LAWS_SLOT
}

pub fn linked_axiom_nodes<'a>(tree: &'a NovelTree, core_id: &str) -> Vec<&'a TreeNode> {
    let mut ids = std::collections::HashSet::new();
    if let Some(core) = tree.nodes.iter().find(|n| n.id == core_id) {
        for id in &core.linked_knowledge_ids {
            ids.insert(id.clone());
        }
    }
    for e in &tree.edges {
        if e.kind != "knowledge" {
            continue;
        }
        let other = if e.source == core_id {
            Some(e.target.as_str())
        } else if e.target == core_id {
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
            if matches!(n.kind, NodeKind::Knowledge) && knowledge_slot(n) == AXIOM_SLOT {
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

fn axiom_has_content(a: &WorldAxiom) -> bool {
    !a.name.trim().is_empty()
        || !a.statement.trim().is_empty()
        || !a.boundary.trim().is_empty()
        || !a.cost.trim().is_empty()
        || !a.mechanism.trim().is_empty()
}

fn push_axiom(lines: &mut Vec<String>, a: &WorldAxiom, idx: usize) {
    let title = if a.name.trim().is_empty() {
        idx.to_string()
    } else {
        a.name.trim().to_string()
    };
    lines.push(format!("### {title}"));
    if !a.statement.trim().is_empty() {
        lines.push(format!("- 表述：{}", a.statement.trim()));
    }
    if !a.boundary.trim().is_empty() {
        lines.push(format!("- 边界：{}", a.boundary.trim()));
    }
    if !a.cost.trim().is_empty() {
        lines.push(format!("- 代价：{}", a.cost.trim()));
    }
    if !a.mechanism.trim().is_empty() {
        lines.push(format!("- 执行机制：{}", a.mechanism.trim()));
    }
    lines.push(String::new());
}

/// 核心法则完整正文：结构化字段 + 公理子卡（优先于可能过期/截断的 extracted）。
pub fn format_core_laws_full(tree: &NovelTree, core: &TreeNode) -> String {
    let Some(kn) = core.knowledge.as_ref() else {
        return core.outline.clone();
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some(cl) = kn.core_laws.as_ref() {
        let premise = cl.premise.trim();
        if !premise.is_empty() {
            lines.push("## 一句话立意".into());
            lines.push(premise.to_string());
            lines.push(String::new());
        }
        let mut axioms: Vec<WorldAxiom> = linked_axiom_nodes(tree, &core.id)
            .iter()
            .filter_map(|n| n.knowledge.as_ref()?.world_axiom.clone())
            .filter(|a| axiom_has_content(a))
            .collect();
        // 兼容未迁移的内嵌 axioms
        if axioms.is_empty() {
            axioms = cl
                .axioms
                .iter()
                .filter(|a| axiom_has_content(a))
                .cloned()
                .collect();
        }
        if !axioms.is_empty() {
            lines.push("## 世界公理".into());
            for (i, a) in axioms.iter().enumerate() {
                push_axiom(&mut lines, a, i + 1);
            }
        }
        let taboos: Vec<&str> = cl
            .taboos
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if !taboos.is_empty() {
            lines.push("## 禁忌红线".into());
            for (i, t) in taboos.iter().enumerate() {
                lines.push(format!("{}. {t}", i + 1));
            }
            lines.push(String::new());
        }
        let ps = cl.power_system.trim();
        if !ps.is_empty() {
            lines.push("## 力量体系".into());
            lines.push(ps.to_string());
            lines.push(String::new());
        }
        let pe = cl.power_expression.trim();
        if !pe.is_empty() {
            lines.push("## 力量表现".into());
            lines.push(pe.to_string());
            lines.push(String::new());
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
    core.outline.trim().to_string()
}

/// 写作/大纲注入用知识卡正文；核心法则 / 时空地理始终拼完整子卡。
pub fn knowledge_card_inject_body(tree: &NovelTree, n: &TreeNode) -> String {
    if is_core_laws_card(n) {
        return format_core_laws_full(tree, n);
    }
    if crate::spatiotemporal_fmt::is_spatiotemporal_card(n) {
        return crate::spatiotemporal_fmt::format_spatiotemporal_full(tree, n);
    }
    if crate::social_power_fmt::is_social_power_card(n) {
        return crate::social_power_fmt::format_social_power_full(tree, n);
    }
    if crate::existence_fmt::is_existence_card(n) {
        return crate::existence_fmt::format_existence_full(n);
    }
    if crate::info_flow_fmt::is_info_flow_card(n) {
        return crate::info_flow_fmt::format_info_flow_full(n);
    }
    if crate::history_culture_fmt::is_history_culture_card(n) {
        return crate::history_culture_fmt::format_history_culture_full(tree, n);
    }
    if crate::story_rules_fmt::is_story_rules_card(n) {
        return crate::story_rules_fmt::format_story_rules_full(tree, n);
    }
    if crate::story_rules_fmt::is_story_rules_fan_card(n) {
        return crate::story_rules_fmt::format_story_rules_fan_full(tree, n);
    }
    let k = n.knowledge.as_ref();
    let feat = k.map(|x| x.extracted.as_str()).unwrap_or("").trim();
    if !feat.is_empty() {
        return feat.to_string();
    }
    let req = k.map(|x| x.extract_prompt.as_str()).unwrap_or("").trim();
    if !req.is_empty() {
        return req.to_string();
    }
    n.outline.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CoreLawsPayload, KnowledgeCardPayload, NodeKind, NodePosition, TreeEdge, TreeNode,
    };

    fn kn(
        id: &str,
        slot: &str,
        core_laws: Option<CoreLawsPayload>,
        world_axiom: Option<WorldAxiom>,
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
                core_laws,
                spatiotemporal: None,
                world_axiom,
                key_location: None,
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
    fn core_laws_inject_includes_linked_axioms() {
        let mut core = kn(
            "core",
            CORE_LAWS_SLOT,
            Some(CoreLawsPayload {
                premise: "力量有价".into(),
                axioms: vec![],
                taboos: vec!["严禁无代价成神".into()],
                power_system: "九境".into(),
                power_expression: "纹路".into(),
            }),
            None,
        );
        core.linked_knowledge_ids = vec!["ax1".into()];
        let ax = kn(
            "ax1",
            AXIOM_SLOT,
            None,
            Some(WorldAxiom {
                name: "借力律".into(),
                statement: "借力必还".into(),
                boundary: "不可永久免除".into(),
                cost: "寿命".into(),
                mechanism: "契约".into(),
            }),
        );
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![core, ax],
            edges: vec![TreeEdge {
                id: "e".into(),
                source: "core".into(),
                target: "ax1".into(),
                kind: "knowledge".into(),
                source_handle: Some("top".into()),
                target_handle: Some("bottom".into()),
                label: String::new(),
            }],
        };
        let body = format_core_laws_full(&tree, &tree.nodes[0]);
        assert!(body.contains("力量有价"));
        assert!(body.contains("### 借力律"));
        assert!(body.contains("借力必还"));
        assert!(body.contains("严禁无代价成神"));
        assert!(!body.contains("stale"));
    }
}
