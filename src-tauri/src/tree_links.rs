//! 根 → 分卷 → 章节：剧情仍继承；知识卡不继承（世界观/故事规则/根或卷直连知识只留在本节点）。

use crate::models::{NodeKind, NovelTree, TreeNode};
use std::collections::HashSet;

pub fn novel_root<'a>(tree: &'a NovelTree) -> Option<&'a TreeNode> {
    tree.nodes.iter().find(|n| matches!(n.kind, NodeKind::Novel))
}

/// `listed` 顺序优先，再并入与 host 相连、同 kind 且尚未列入的节点。
pub fn union_linked_ids(
    tree: &NovelTree,
    host_id: &str,
    listed: &[String],
    kind: NodeKind,
) -> Vec<String> {
    let mut ids = listed.to_vec();
    for e in &tree.edges {
        if e.source != host_id && e.target != host_id {
            continue;
        }
        let other = if e.source == host_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        let Some(n) = tree.nodes.iter().find(|n| n.id == other) else {
            continue;
        };
        if n.kind == kind && !ids.iter().any(|id| id == other) {
            ids.push(other.to_string());
        }
    }
    ids
}

pub fn root_plot_ids(tree: &NovelTree) -> Vec<String> {
    let Some(root) = novel_root(tree) else {
        return vec![];
    };
    union_linked_ids(
        tree,
        &root.id,
        &root.linked_side_plot_ids,
        NodeKind::SidePlot,
    )
}

pub fn root_knowledge_ids(tree: &NovelTree) -> Vec<String> {
    let Some(root) = novel_root(tree) else {
        return vec![];
    };
    union_linked_ids(
        tree,
        &root.id,
        &root.linked_knowledge_ids,
        NodeKind::Knowledge,
    )
    .into_iter()
    .filter(|id| !crate::write_prompts::is_write_prompt_excluded_id(tree, id))
    .collect()
}

pub fn root_character_ids(tree: &NovelTree) -> Vec<String> {
    let Some(root) = novel_root(tree) else {
        return vec![];
    };
    union_linked_ids(
        tree,
        &root.id,
        &root.linked_character_ids,
        NodeKind::Character,
    )
}

fn host_linked(
    tree: &NovelTree,
    host_id: &str,
    link_field: fn(&TreeNode) -> &[String],
    kind: NodeKind,
) -> Vec<String> {
    let listed = tree
        .nodes
        .iter()
        .find(|n| n.id == host_id)
        .map(link_field)
        .unwrap_or(&[]);
    union_linked_ids(tree, host_id, listed, kind)
}

/// 沿 spine 边上溯找父分卷；无则 None。
/// - 边方向兼容（父→子 / 子→父）
/// - 与分卷任意相连即认作父卷（不依赖边 kind，避免拖线 kind 判错）
/// - 同层兼挂根时优先 Volume，不被 Novel 短路
pub fn chapter_parent_volume_id(tree: &NovelTree, chapter_id: &str) -> Option<String> {
    let by_id: std::collections::HashMap<&str, &TreeNode> =
        tree.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut cur = chapter_id.to_string();
    let mut seen = HashSet::new();
    while seen.insert(cur.clone()) {
        let mut volume: Option<String> = None;
        let mut chapter_parent: Option<String> = None;
        let mut hit_novel = false;
        for e in &tree.edges {
            // 卡类边不参与主轴上溯
            if e.kind == "character" || e.kind == "side_plot" || e.kind == "knowledge" {
                continue;
            }
            let other = if e.target == cur {
                e.source.as_str()
            } else if e.source == cur {
                e.target.as_str()
            } else {
                continue;
            };
            let Some(n) = by_id.get(other) else {
                continue;
            };
            match n.kind {
                NodeKind::Volume => {
                    if volume.is_none() {
                        volume = Some(n.id.clone());
                    }
                }
                NodeKind::Novel => hit_novel = true,
                NodeKind::Chapter => {
                    if chapter_parent.is_none() {
                        chapter_parent = Some(other.to_string());
                    }
                }
                _ => {}
            }
        }
        if let Some(vid) = volume {
            return Some(vid);
        }
        if let Some(p) = chapter_parent {
            cur = p;
            continue;
        }
        if hit_novel {
            return None;
        }
        return None;
    }
    None
}

pub fn volume_plot_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    host_linked(tree, volume_id, |n| &n.linked_side_plot_ids, NodeKind::SidePlot)
}

pub fn volume_knowledge_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    host_linked(tree, volume_id, |n| &n.linked_knowledge_ids, NodeKind::Knowledge)
}

pub fn volume_character_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    host_linked(tree, volume_id, |n| &n.linked_character_ids, NodeKind::Character)
}

/// 章节继承的剧情 id：根 + 父分卷（若有）。
pub fn chapter_inherited_plot_ids(tree: &NovelTree, chapter_id: &str) -> Vec<String> {
    let mut ids = root_plot_ids(tree);
    if let Some(vid) = chapter_parent_volume_id(tree, chapter_id) {
        for pid in volume_plot_ids(tree, &vid) {
            if !ids.iter().any(|id| id == &pid) {
                ids.push(pid);
            }
        }
    }
    ids
}

/// 章节本机剧情（不含根/分卷继承）。
pub fn chapter_local_plot_ids(tree: &NovelTree, chapter_id: &str) -> Vec<String> {
    let inherited: HashSet<String> = chapter_inherited_plot_ids(tree, chapter_id)
        .into_iter()
        .collect();
    let node = tree.nodes.iter().find(|n| n.id == chapter_id);
    let mut ids: Vec<String> = node
        .map(|n| {
            n.linked_side_plot_ids
                .iter()
                .filter(|id| !inherited.contains(*id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    for e in &tree.edges {
        if e.source != chapter_id && e.target != chapter_id {
            continue;
        }
        let other = if e.source == chapter_id {
            e.target.as_str()
        } else {
            e.source.as_str()
        };
        if inherited.contains(other) {
            continue;
        }
        let Some(n) = tree.nodes.iter().find(|n| n.id == other) else {
            continue;
        };
        if matches!(n.kind, NodeKind::SidePlot) && !ids.iter().any(|id| id == other) {
            ids.push(other.to_string());
        }
    }
    ids
}

/// 知识卡不向章/卷继承。
pub fn chapter_inherited_knowledge_ids(_tree: &NovelTree, _chapter_id: &str) -> Vec<String> {
    vec![]
}

/// 章节直连知识卡。
pub fn chapter_local_knowledge_ids(tree: &NovelTree, chapter_id: &str) -> Vec<String> {
    let listed = tree
        .nodes
        .iter()
        .find(|n| n.id == chapter_id)
        .map(|n| n.linked_knowledge_ids.as_slice())
        .unwrap_or(&[]);
    union_linked_ids(tree, chapter_id, listed, NodeKind::Knowledge)
}

/// 分卷直连知识（不含根上的卡，除非本卷也直连）。
pub fn volume_local_knowledge_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    volume_knowledge_ids(tree, volume_id)
}

/// 章节面板/MCP 章快照用的知识：仅本章直连。
pub fn chapter_effective_knowledge_ids(tree: &NovelTree, chapter_id: &str) -> Vec<String> {
    chapter_local_knowledge_ids(tree, chapter_id)
}

/// 写章注入：根 ∪ 卷直连 ∪ 章直连（去重，先到优先）。
pub fn chapter_write_knowledge_ids(tree: &NovelTree, chapter_id: &str) -> Vec<String> {
    let mut ids = root_knowledge_ids(tree);
    if let Some(vid) = chapter_parent_volume_id(tree, chapter_id) {
        for kid in volume_local_knowledge_ids(tree, &vid) {
            if !ids.iter().any(|id| id == &kid) {
                ids.push(kid);
            }
        }
    }
    for kid in chapter_local_knowledge_ids(tree, chapter_id) {
        if !ids.iter().any(|id| id == &kid) {
            ids.push(kid);
        }
    }
    ids
        .into_iter()
        .filter(|id| !crate::write_prompts::is_write_prompt_excluded_id(tree, id))
        .collect()
}

/// 分卷面板用的知识：仅本卷直连。
pub fn volume_effective_knowledge_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    volume_local_knowledge_ids(tree, volume_id)
}

/// 分卷本机剧情（不含根继承）。
pub fn volume_local_plot_ids(tree: &NovelTree, volume_id: &str) -> Vec<String> {
    let root_set: HashSet<String> = root_plot_ids(tree).into_iter().collect();
    volume_plot_ids(tree, volume_id)
        .into_iter()
        .filter(|id| !root_set.contains(id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NodePosition, NovelTree, TreeEdge, TreeNode};

    fn node(id: &str, kind: NodeKind, plots: &[&str], know: &[&str]) -> TreeNode {
        TreeNode {
            id: id.into(),
            kind,
            label: id.into(),
            outline: String::new(),
            detailed_outline: vec![],
            character: None,
            knowledge: None,
            side_plot: None,
            volume: None,
            linked_character_ids: vec![],
            linked_side_plot_ids: plots.iter().map(|s| (*s).into()).collect(),
            linked_knowledge_ids: know.iter().map(|s| (*s).into()).collect(),
            position: NodePosition { x: 0.0, y: 0.0 },
            word_count: 0,
            word_count_min: 0,
            word_count_max: 0,
            chapter_count: 0,
        }
    }

    fn edge(id: &str, source: &str, target: &str, kind: &str) -> TreeEdge {
        TreeEdge {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            kind: kind.into(),
            source_handle: None,
            target_handle: None,
            label: String::new(),
        }
    }

    #[test]
    fn chapter_local_plot_excludes_root_inherited() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &["rp"], &["rk"]),
                node("ch", NodeKind::Chapter, &["rp", "cp2", "cp1"], &[]),
                node("rp", NodeKind::SidePlot, &[], &[]),
                node("cp1", NodeKind::SidePlot, &[], &[]),
                node("cp2", NodeKind::SidePlot, &[], &[]),
            ],
            edges: vec![],
        };
        assert_eq!(chapter_local_plot_ids(&tree, "ch"), vec!["cp2", "cp1"]);
    }

    #[test]
    fn chapter_effective_knowledge_is_local_only() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &[], &["rk"]),
                node("ch", NodeKind::Chapter, &[], &["ck"]),
                node("rk", NodeKind::Knowledge, &[], &[]),
                node("ck", NodeKind::Knowledge, &[], &[]),
            ],
            edges: vec![],
        };
        assert!(chapter_inherited_knowledge_ids(&tree, "ch").is_empty());
        assert_eq!(chapter_effective_knowledge_ids(&tree, "ch"), vec!["ck"]);
        assert_eq!(
            chapter_write_knowledge_ids(&tree, "ch"),
            vec!["rk", "ck"]
        );
    }

    #[test]
    fn chapter_inherits_volume_then_root() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &["rp"], &["rk"]),
                node("vol", NodeKind::Volume, &["vp"], &["vk"]),
                node("ch", NodeKind::Chapter, &["cp"], &["ck"]),
                node("rp", NodeKind::SidePlot, &[], &[]),
                node("vp", NodeKind::SidePlot, &[], &[]),
                node("cp", NodeKind::SidePlot, &[], &[]),
                node("rk", NodeKind::Knowledge, &[], &[]),
                node("vk", NodeKind::Knowledge, &[], &[]),
                node("ck", NodeKind::Knowledge, &[], &[]),
            ],
            edges: vec![
                edge("e-rv", "root", "vol", "volume"),
                edge("e-vc", "vol", "ch", "chapter"),
            ],
        };
        assert_eq!(chapter_parent_volume_id(&tree, "ch").as_deref(), Some("vol"));
        assert_eq!(chapter_local_plot_ids(&tree, "ch"), vec!["cp"]);
        assert_eq!(volume_local_plot_ids(&tree, "vol"), vec!["vp"]);
        assert_eq!(volume_effective_knowledge_ids(&tree, "vol"), vec!["vk"]);
        let inherited = chapter_inherited_plot_ids(&tree, "ch");
        assert!(inherited.iter().any(|id| id == "rp"));
        assert!(inherited.iter().any(|id| id == "vp"));
        assert_eq!(chapter_effective_knowledge_ids(&tree, "ch"), vec!["ck"]);
        assert_eq!(
            chapter_write_knowledge_ids(&tree, "ch"),
            vec!["rk", "vk", "ck"]
        );
    }

    #[test]
    fn chapter_parent_volume_accepts_reverse_edge() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &[], &[]),
                node("vol", NodeKind::Volume, &["vp"], &[]),
                node("ch", NodeKind::Chapter, &["cp"], &[]),
                node("vp", NodeKind::SidePlot, &[], &[]),
                node("cp", NodeKind::SidePlot, &[], &[]),
            ],
            // 子→父（画布拖线可能颠倒）
            edges: vec![edge("e-cv", "ch", "vol", "chapter")],
        };
        assert_eq!(chapter_parent_volume_id(&tree, "ch").as_deref(), Some("vol"));
        assert_eq!(chapter_local_plot_ids(&tree, "ch"), vec!["cp"]);
        assert!(chapter_inherited_plot_ids(&tree, "ch").contains(&"vp".into()));
    }

    #[test]
    fn chapter_parent_volume_prefers_volume_over_root_edge() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &[], &[]),
                node("vol", NodeKind::Volume, &["vp"], &[]),
                node("ch", NodeKind::Chapter, &[], &[]),
                node("vp", NodeKind::SidePlot, &[], &[]),
            ],
            // 章同时挂根与分卷时，仍应认分卷
            edges: vec![
                edge("e-rc", "root", "ch", "chapter"),
                edge("e-vc", "vol", "ch", "chapter"),
                edge("e-rv", "root", "vol", "volume"),
            ],
        };
        assert_eq!(chapter_parent_volume_id(&tree, "ch").as_deref(), Some("vol"));
        assert!(chapter_inherited_plot_ids(&tree, "ch").contains(&"vp".into()));
    }

    #[test]
    fn chapter_without_volume_skips_volume_inherit() {
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &[], &["rk"]),
                node("vol", NodeKind::Volume, &[], &["vk"]),
                node("ch", NodeKind::Chapter, &[], &["ck"]),
                node("rk", NodeKind::Knowledge, &[], &[]),
                node("vk", NodeKind::Knowledge, &[], &[]),
                node("ck", NodeKind::Knowledge, &[], &[]),
            ],
            edges: vec![
                edge("e-rv", "root", "vol", "volume"),
                edge("e-rc", "root", "ch", "chapter"),
            ],
        };
        assert!(chapter_parent_volume_id(&tree, "ch").is_none());
        assert_eq!(chapter_effective_knowledge_ids(&tree, "ch"), vec!["ck"]);
        assert_eq!(
            chapter_write_knowledge_ids(&tree, "ch"),
            vec!["rk", "ck"]
        );
    }

    #[test]
    fn write_prompts_excluded_from_inherit() {
        use crate::models::KnowledgeCardPayload;
        let mut hub = node("hub", NodeKind::Knowledge, &[], &["g1"]);
        hub.knowledge = Some(KnowledgeCardPayload {
            slot: crate::write_prompts::WRITE_PROMPTS_SLOT.into(),
            ..Default::default()
        });
        let tree = NovelTree {
            novel_id: "n".into(),
            nodes: vec![
                node("root", NodeKind::Novel, &[], &["hub", "rk"]),
                hub,
                node("g1", NodeKind::Knowledge, &[], &[]),
                node("rk", NodeKind::Knowledge, &[], &[]),
                node("ch", NodeKind::Chapter, &[], &["ck"]),
                node("ck", NodeKind::Knowledge, &[], &[]),
            ],
            edges: vec![
                edge("e-root-hub", "root", "hub", "knowledge"),
                edge("e-hub-g", "hub", "g1", "knowledge"),
            ],
        };
        assert_eq!(root_knowledge_ids(&tree), vec!["rk"]);
        assert_eq!(chapter_write_knowledge_ids(&tree, "ch"), vec!["rk", "ck"]);
    }
}
