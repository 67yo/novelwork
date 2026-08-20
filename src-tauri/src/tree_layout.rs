//! 与 `src/lib/treeLayout.ts` 同一套一键排版（无 Vue 实测高度，用 kind 估高）。
//! ponytail: 双份算法，改间距时两边一起改。
use crate::models::{NodeKind, NodePosition, NovelTree, TreeEdge, TreeNode};
use crate::tree_links;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

const CHAR_X: f64 = 40.0;
const MAIN_X: f64 = 320.0;
const PLOT_X: f64 = 640.0;
const PLOT_DX: f64 = 220.0;
const PLOT_CARD_W: f64 = 200.0;
const PLOT_DY: f64 = 100.0;
const MAX_PLOT_COL: usize = 4;
const SIDE_DX: f64 = 200.0;
const SIDE_CARD_W: f64 = 200.0;
const SIDE_GAP: f64 = 12.0;
const MAX_SIDE_COL: usize = 4;
const KNOW_GAP_FROM_CHAR: f64 = SIDE_CARD_W + SIDE_CARD_W / 2.0;
const TOP: f64 = 40.0;
const MIN_CHAPTER_GAP_TIGHT: f64 = 72.0;
const MIN_CHAPTER_GAP: f64 = 200.0;
const CH_PAD: f64 = 48.0;

fn default_node_h(kind: &NodeKind) -> f64 {
    match kind {
        NodeKind::Character => 96.0,
        NodeKind::Knowledge => 88.0,
        NodeKind::SidePlot => 72.0,
        NodeKind::Novel => 100.0,
        NodeKind::Volume => 88.0,
        NodeKind::Chapter => 64.0,
    }
}

fn by_y(a: &TreeNode, b: &TreeNode) -> Ordering {
    a.position
        .y
        .partial_cmp(&b.position.y)
        .unwrap_or(Ordering::Equal)
        .then(
            a.position
                .x
                .partial_cmp(&b.position.x)
                .unwrap_or(Ordering::Equal),
        )
        .then(a.id.cmp(&b.id))
}

fn place_grid_ids(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    ids: &[String],
    origin_x: f64,
    origin_y: f64,
    dx: f64,
    dy: f64,
    max_col: usize,
    dir: f64,
) {
    for (i, id) in ids.iter().enumerate() {
        let col = i / max_col;
        let row = i % max_col;
        let idx = *by_id.get(id).unwrap();
        nodes[idx].position = NodePosition {
            x: origin_x + dir * col as f64 * dx,
            y: origin_y + row as f64 * dy,
        };
    }
}

fn place_side_ids(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    ids: &[String],
    origin_x: f64,
    origin_y: f64,
    dx: f64,
    max_col: usize,
    dir: f64,
) -> f64 {
    if ids.is_empty() {
        return 0.0;
    }
    let mut next_y: Vec<f64> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut max_bottom = origin_y;
    for id in ids {
        let mut col = 0usize;
        while *counts.get(col).unwrap_or(&0) >= max_col {
            col += 1;
        }
        while next_y.len() <= col {
            next_y.push(origin_y);
            counts.push(0);
        }
        let y = next_y[col];
        let idx = *by_id.get(id).unwrap();
        nodes[idx].position = NodePosition {
            x: origin_x + dir * col as f64 * dx,
            y,
        };
        let h = default_node_h(&nodes[idx].kind);
        let bottom = y + h;
        next_y[col] = bottom + SIDE_GAP;
        counts[col] += 1;
        max_bottom = max_bottom.max(bottom);
    }
    (max_bottom - origin_y).max(0.0)
}

fn place_know_ids(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    know_ids: &[String],
    char_ids: &[String],
    origin_y: f64,
) -> f64 {
    if know_ids.is_empty() {
        return 0.0;
    }
    let left_char_x = char_ids
        .iter()
        .filter_map(|id| by_id.get(id).map(|&i| nodes[i].position.x))
        .fold(f64::INFINITY, f64::min);
    let origin_x = if left_char_x.is_finite() {
        left_char_x - KNOW_GAP_FROM_CHAR
    } else {
        CHAR_X - KNOW_GAP_FROM_CHAR
    };
    place_side_ids(nodes, by_id, know_ids, origin_x, origin_y, SIDE_DX, MAX_SIDE_COL, -1.0)
}

fn grid_span(count: usize, dy: f64, max_col: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        (count.min(max_col) as f64) * dy
    }
}

fn plot_overlaps(ax: f64, ay: f64, bx: f64, by: f64) -> bool {
    (ax - bx).abs() < PLOT_DX * 0.9 && (ay - by).abs() < PLOT_DY * 0.9
}

fn is_host_kind(k: &NodeKind) -> bool {
    matches!(
        k,
        NodeKind::Novel | NodeKind::Volume | NodeKind::Chapter | NodeKind::SidePlot
    )
}

fn host_rank_lt(nodes: &[TreeNode], a_id: &str, b_id: &str, by_id: &HashMap<String, usize>) -> bool {
    let a = &nodes[*by_id.get(a_id).unwrap()];
    let b = &nodes[*by_id.get(b_id).unwrap()];
    let kr = |k: &NodeKind| match k {
        NodeKind::Chapter => 0u8,
        NodeKind::Volume => 1,
        NodeKind::Novel => 2,
        _ => 3,
    };
    match kr(&b.kind).cmp(&kr(&a.kind)) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => match b.position.y.partial_cmp(&a.position.y).unwrap_or(Ordering::Equal) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => match b.position.x.partial_cmp(&a.position.x).unwrap_or(Ordering::Equal)
            {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => b.id.as_str() < a.id.as_str(),
            },
        },
    }
}

fn consider_host(
    host: &mut HashMap<String, String>,
    by_id: &HashMap<String, usize>,
    nodes: &[TreeNode],
    card_id: &str,
    host_id: &str,
    card_kind: NodeKind,
) {
    let Some(&hi) = by_id.get(host_id) else { return };
    let Some(&ci) = by_id.get(card_id) else { return };
    let h = &nodes[hi];
    let c = &nodes[ci];
    if c.kind != card_kind || !is_host_kind(&h.kind) {
        return;
    }
    match host.get(card_id) {
        None => {
            host.insert(card_id.to_string(), host_id.to_string());
        }
        Some(prev) => {
            if host_rank_lt(nodes, prev, host_id, by_id) {
                host.insert(card_id.to_string(), host_id.to_string());
            }
        }
    }
}

fn resolve_hosts(
    nodes: &[TreeNode],
    edges: &[TreeEdge],
    card_kind: NodeKind,
    link: impl Fn(&TreeNode) -> &[String],
    edge_kind: &str,
) -> HashMap<String, String> {
    let by_id: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();
    let mut host = HashMap::new();
    for n in nodes {
        for id in link(n) {
            consider_host(&mut host, &by_id, nodes, id, &n.id, card_kind);
        }
    }
    for e in edges {
        if e.kind != edge_kind {
            continue;
        }
        let Some(&si) = by_id.get(&e.source) else { continue };
        let Some(&ti) = by_id.get(&e.target) else { continue };
        let s = &nodes[si];
        let t = &nodes[ti];
        if s.kind == card_kind {
            consider_host(&mut host, &by_id, nodes, &s.id, &t.id, card_kind);
        }
        if t.kind == card_kind {
            consider_host(&mut host, &by_id, nodes, &t.id, &s.id, card_kind);
        }
    }
    host
}

fn lift_side_plot_hosts(
    host: &mut HashMap<String, String>,
    plot_primary: &HashMap<String, String>,
    nodes: &[TreeNode],
    by_id: &HashMap<String, usize>,
) {
    let updates: Vec<(String, String)> = host
        .iter()
        .filter_map(|(card_id, hid)| {
            let i = *by_id.get(hid)?;
            if nodes[i].kind == NodeKind::SidePlot {
                plot_primary
                    .get(hid)
                    .map(|ch| (card_id.clone(), ch.clone()))
            } else {
                None
            }
        })
        .collect();
    for (k, v) in updates {
        host.insert(k, v);
    }
}

fn collect_plot_hosts(
    hosts: &[&TreeNode],
    edges: &[TreeEdge],
    nodes: &[TreeNode],
    by_id: &HashMap<String, usize>,
) -> HashMap<String, Vec<String>> {
    let host_ids: HashSet<&str> = hosts.iter().map(|h| h.id.as_str()).collect();
    let mut map: HashMap<String, HashSet<String>> = HashMap::new();
    let mut add = |plot_id: &str, host_id: &str| {
        if !host_ids.contains(host_id) {
            return;
        }
        let Some(&pi) = by_id.get(plot_id) else { return };
        if nodes[pi].kind != NodeKind::SidePlot {
            return;
        }
        map.entry(plot_id.to_string())
            .or_default()
            .insert(host_id.to_string());
    };
    for h in hosts {
        for pid in &h.linked_side_plot_ids {
            add(pid, &h.id);
        }
    }
    for e in edges {
        if e.kind != "side_plot" {
            continue;
        }
        let Some(&si) = by_id.get(&e.source) else { continue };
        let Some(&ti) = by_id.get(&e.target) else { continue };
        let s = &nodes[si];
        let t = &nodes[ti];
        let s_host = matches!(s.kind, NodeKind::Chapter | NodeKind::Novel | NodeKind::Volume);
        let t_host = matches!(t.kind, NodeKind::Chapter | NodeKind::Novel | NodeKind::Volume);
        if s_host && t.kind == NodeKind::SidePlot {
            add(&t.id, &s.id);
        } else if t_host && s.kind == NodeKind::SidePlot {
            add(&s.id, &t.id);
        }
    }
    let mut out = HashMap::new();
    for (pid, set) in map {
        let mut list: Vec<String> = set.into_iter().collect();
        list.sort_by(|a, b| {
            let ca = &nodes[*by_id.get(a).unwrap()];
            let cb = &nodes[*by_id.get(b).unwrap()];
            by_y(ca, cb)
        });
        out.insert(pid, list);
    }
    out
}

fn host_has_linked(
    host: &TreeNode,
    plot_hosts: &HashMap<String, Vec<String>>,
    plots_by_host: &HashMap<String, Vec<String>>,
    chars_by_host: &HashMap<String, Vec<String>>,
    knows_by_host: &HashMap<String, Vec<String>>,
) -> bool {
    if plots_by_host.get(&host.id).is_some_and(|v| !v.is_empty()) {
        return true;
    }
    if chars_by_host.get(&host.id).is_some_and(|v| !v.is_empty()) {
        return true;
    }
    if knows_by_host.get(&host.id).is_some_and(|v| !v.is_empty()) {
        return true;
    }
    for chs in plot_hosts.values() {
        if chs.iter().any(|id| id == &host.id) {
            return true;
        }
    }
    !host.linked_side_plot_ids.is_empty()
        || !host.linked_character_ids.is_empty()
        || !host.linked_knowledge_ids.is_empty()
}

fn sort_plots_for_host(host: &TreeNode, plots: &mut [String], by_id: &HashMap<String, usize>, nodes: &[TreeNode]) {
    let order = &host.linked_side_plot_ids;
    let idx: HashMap<&str, usize> = order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    plots.sort_by(|a, b| {
        let ia = idx.get(a.as_str()).copied().unwrap_or(order.len());
        let ib = idx.get(b.as_str()).copied().unwrap_or(order.len());
        ia.cmp(&ib).then_with(|| {
            let na = &nodes[*by_id.get(a).unwrap()];
            let nb = &nodes[*by_id.get(b).unwrap()];
            by_y(na, nb).then(a.cmp(b))
        })
    });
}

fn is_spine_edge(kind: &str) -> bool {
    kind != "character" && kind != "side_plot" && kind != "knowledge"
}

fn spine_neighbors(id: &str, edges: &[TreeEdge], want: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for e in edges {
        if !is_spine_edge(&e.kind) {
            continue;
        }
        let other = if e.source == id {
            e.target.as_str()
        } else if e.target == id {
            e.source.as_str()
        } else {
            continue;
        };
        if want.contains(other) && !out.iter().any(|x| x == other) {
            out.push(other.to_string());
        }
    }
    out
}

fn walk_volume_chain(
    vid: &str,
    vol_order: &mut Vec<String>,
    vol_seen: &mut HashSet<String>,
    tree: &NovelTree,
    by_id: &HashMap<String, usize>,
    vol_ids: &HashSet<String>,
) {
    if !vol_seen.insert(vid.to_string()) {
        return;
    }
    vol_order.push(vid.to_string());
    let mut next = spine_neighbors(vid, &tree.edges, vol_ids);
    next.retain(|id| !vol_seen.contains(id));
    next.sort_by(|a, b| {
        by_y(
            &tree.nodes[*by_id.get(a).unwrap()],
            &tree.nodes[*by_id.get(b).unwrap()],
        )
    });
    for n in next {
        walk_volume_chain(&n, vol_order, vol_seen, tree, by_id, vol_ids);
    }
}

fn order_chapter_group_ids(
    chs: &[&TreeNode],
    tree: &NovelTree,
    by_id: &HashMap<String, usize>,
) -> Vec<String> {
    if chs.len() <= 1 {
        return chs.iter().map(|c| c.id.clone()).collect();
    }
    let ids: HashSet<String> = chs.iter().map(|c| c.id.clone()).collect();
    let mut succ: HashMap<String, Vec<String>> = HashMap::new();
    let mut pred_count: HashMap<String, usize> = HashMap::new();
    for c in chs {
        pred_count.insert(c.id.clone(), 0);
    }
    for e in &tree.edges {
        if !is_spine_edge(&e.kind) {
            continue;
        }
        if !ids.contains(&e.source) || !ids.contains(&e.target) {
            continue;
        }
        let list = succ.entry(e.source.clone()).or_default();
        if !list.iter().any(|x| x == &e.target) {
            list.push(e.target.clone());
            *pred_count.entry(e.target.clone()).or_default() += 1;
        }
    }
    let mut starts: Vec<&TreeNode> = chs
        .iter()
        .copied()
        .filter(|c| pred_count.get(&c.id).copied().unwrap_or(0) == 0)
        .collect();
    starts.sort_by(|a, b| by_y(a, b));
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    fn walk_ch(
        id: &str,
        ordered: &mut Vec<String>,
        seen: &mut HashSet<String>,
        succ: &HashMap<String, Vec<String>>,
        tree: &NovelTree,
        by_id: &HashMap<String, usize>,
    ) {
        if !seen.insert(id.to_string()) {
            return;
        }
        ordered.push(id.to_string());
        let mut next = succ.get(id).cloned().unwrap_or_default();
        next.retain(|x| !seen.contains(x));
        next.sort_by(|a, b| {
            by_y(
                &tree.nodes[*by_id.get(a).unwrap()],
                &tree.nodes[*by_id.get(b).unwrap()],
            )
        });
        for x in next {
            walk_ch(&x, ordered, seen, succ, tree, by_id);
        }
    }
    for s in starts {
        walk_ch(&s.id, &mut ordered, &mut seen, &succ, tree, by_id);
    }
    let mut rest: Vec<&TreeNode> = chs.to_vec();
    rest.sort_by(|a, b| by_y(a, b));
    for c in rest {
        walk_ch(&c.id, &mut ordered, &mut seen, &succ, tree, by_id);
    }
    ordered
}

/// 主轴：根 →（分卷 → 该卷下章节）* → 无分卷章节
fn build_spine_ids(tree: &NovelTree, by_id: &HashMap<String, usize>) -> Vec<String> {
    let root = tree.nodes.iter().find(|n| matches!(n.kind, NodeKind::Novel));
    let mut volumes: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Volume))
        .collect();
    let chapters: Vec<&TreeNode> = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Chapter))
        .collect();

    let vol_ids: HashSet<String> = volumes.iter().map(|v| v.id.clone()).collect();
    let mut vol_order: Vec<String> = Vec::new();
    let mut vol_seen: HashSet<String> = HashSet::new();

    if let Some(root) = root {
        let mut from_root = spine_neighbors(&root.id, &tree.edges, &vol_ids);
        from_root.sort_by(|a, b| {
            by_y(
                &tree.nodes[*by_id.get(a).unwrap()],
                &tree.nodes[*by_id.get(b).unwrap()],
            )
        });
        for id in from_root {
            walk_volume_chain(&id, &mut vol_order, &mut vol_seen, tree, by_id, &vol_ids);
        }
    }
    volumes.sort_by(|a, b| by_y(a, b));
    for v in &volumes {
        walk_volume_chain(&v.id, &mut vol_order, &mut vol_seen, tree, by_id, &vol_ids);
    }

    let vol_set: HashSet<String> = vol_order.iter().cloned().collect();
    let mut under_vol: HashMap<String, Vec<&TreeNode>> = HashMap::new();
    let mut free_chapters: Vec<&TreeNode> = Vec::new();
    for ch in &chapters {
        match tree_links::chapter_parent_volume_id(tree, &ch.id) {
            Some(vid) if vol_set.contains(&vid) => {
                under_vol.entry(vid).or_default().push(*ch);
            }
            _ => free_chapters.push(*ch),
        }
    }

    let mut spine = Vec::new();
    if let Some(root) = root {
        spine.push(root.id.clone());
    }
    for vid in &vol_order {
        spine.push(vid.clone());
        let chs = under_vol.get(vid).map(|v| v.as_slice()).unwrap_or(&[]);
        spine.extend(order_chapter_group_ids(chs, tree, by_id));
    }
    spine.extend(order_chapter_group_ids(&free_chapters, tree, by_id));
    spine
}

pub fn apply_auto_layout(tree: &mut NovelTree) {
    if tree.nodes.is_empty() {
        return;
    }
    let by_id: HashMap<String, usize> = tree
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();
    let spine_ids = build_spine_ids(tree, &by_id);
    let spine_refs: Vec<&TreeNode> = spine_ids
        .iter()
        .map(|id| &tree.nodes[*by_id.get(id).unwrap()])
        .collect();
    let plot_hosts = collect_plot_hosts(&spine_refs, &tree.edges, &tree.nodes, &by_id);
    let plot_primary: HashMap<String, String> = plot_hosts
        .iter()
        .filter_map(|(pid, chs)| chs.first().map(|c| (pid.clone(), c.clone())))
        .collect();

    let mut char_host = resolve_hosts(
        &tree.nodes,
        &tree.edges,
        NodeKind::Character,
        |n| &n.linked_character_ids,
        "character",
    );
    let mut know_host = resolve_hosts(
        &tree.nodes,
        &tree.edges,
        NodeKind::Knowledge,
        |n| &n.linked_knowledge_ids,
        "knowledge",
    );
    lift_side_plot_hosts(&mut char_host, &plot_primary, &tree.nodes, &by_id);
    lift_side_plot_hosts(&mut know_host, &plot_primary, &tree.nodes, &by_id);

    let spine_set: HashSet<String> = spine_ids.iter().cloned().collect();
    let mut plots_by_host: HashMap<String, Vec<String>> = HashMap::new();
    let mut multi_plots: Vec<String> = Vec::new();
    let mut orphan_plots: Vec<String> = Vec::new();
    for p in tree.nodes.iter().filter(|n| n.kind == NodeKind::SidePlot) {
        let chs = plot_hosts.get(&p.id).map(|v| v.as_slice()).unwrap_or(&[]);
        if chs.len() >= 2 {
            multi_plots.push(p.id.clone());
        } else if chs.len() == 1 {
            plots_by_host
                .entry(chs[0].clone())
                .or_default()
                .push(p.id.clone());
        } else {
            orphan_plots.push(p.id.clone());
        }
    }

    let mut chars_by_host: HashMap<String, Vec<String>> = HashMap::new();
    let mut knows_by_host: HashMap<String, Vec<String>> = HashMap::new();
    let mut orphan_chars: Vec<String> = Vec::new();
    let mut orphan_knows: Vec<String> = Vec::new();
    for n in &tree.nodes {
        let (host_map, by_host, orphans) = match n.kind {
            NodeKind::Character => (&char_host, &mut chars_by_host, &mut orphan_chars),
            NodeKind::Knowledge => (&know_host, &mut knows_by_host, &mut orphan_knows),
            _ => continue,
        };
        if let Some(hid) = host_map.get(&n.id) {
            if spine_set.contains(hid) {
                by_host.entry(hid.clone()).or_default().push(n.id.clone());
                continue;
            }
        }
        orphans.push(n.id.clone());
    }
    for list in chars_by_host.values_mut() {
        list.sort_by(|a, b| {
            by_y(
                &tree.nodes[*by_id.get(a).unwrap()],
                &tree.nodes[*by_id.get(b).unwrap()],
            )
        });
    }
    for list in knows_by_host.values_mut() {
        list.sort_by(|a, b| {
            by_y(
                &tree.nodes[*by_id.get(a).unwrap()],
                &tree.nodes[*by_id.get(b).unwrap()],
            )
        });
    }

    let mut occupied: Vec<(f64, f64)> = Vec::new();
    let mut y = TOP;
    for hid in &spine_ids {
        let hi = *by_id.get(hid).unwrap();
        tree.nodes[hi].position = NodePosition { x: MAIN_X, y };
        if let Some(plots) = plots_by_host.get_mut(hid) {
            sort_plots_for_host(&tree.nodes[hi], plots, &by_id, &tree.nodes);
        }
        let plot_ids = plots_by_host.get(hid).cloned().unwrap_or_default();
        let char_ids = chars_by_host.get(hid).cloned().unwrap_or_default();
        let know_ids = knows_by_host.get(hid).cloned().unwrap_or_default();

        place_grid_ids(
            &mut tree.nodes,
            &by_id,
            &plot_ids,
            PLOT_X,
            y,
            PLOT_DX,
            PLOT_DY,
            MAX_PLOT_COL,
            1.0,
        );
        for id in &plot_ids {
            let p = &tree.nodes[*by_id.get(id).unwrap()].position;
            occupied.push((p.x, p.y));
        }

        let char_span = place_side_ids(
            &mut tree.nodes,
            &by_id,
            &char_ids,
            CHAR_X,
            y,
            SIDE_DX,
            MAX_SIDE_COL,
            -1.0,
        );
        let know_span = place_know_ids(&mut tree.nodes, &by_id, &know_ids, &char_ids, y);
        let span = grid_span(plot_ids.len(), PLOT_DY, MAX_PLOT_COL)
            .max(char_span)
            .max(know_span);
        let linked = host_has_linked(
            &tree.nodes[hi],
            &plot_hosts,
            &plots_by_host,
            &chars_by_host,
            &knows_by_host,
        );
        let step = if linked {
            MIN_CHAPTER_GAP.max(span + CH_PAD)
        } else {
            MIN_CHAPTER_GAP_TIGHT.max(default_node_h(&tree.nodes[hi].kind) + SIDE_GAP)
        };
        y = tree.nodes[hi].position.y + step;
    }

    multi_plots.sort_by(|a, b| {
        let mid = |id: &str| {
            let chs = plot_hosts.get(id).map(|v| v.as_slice()).unwrap_or(&[]);
            let ys: Vec<f64> = chs
                .iter()
                .filter_map(|cid| by_id.get(cid).map(|&i| tree.nodes[i].position.y))
                .collect();
            if ys.is_empty() {
                tree.nodes[*by_id.get(id).unwrap()].position.y
            } else {
                (ys.iter().copied().fold(f64::INFINITY, f64::min)
                    + ys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                    / 2.0
            }
        };
        mid(a).partial_cmp(&mid(b)).unwrap_or(Ordering::Equal).then(a.cmp(b))
    });
    for pid in &multi_plots {
        let ch_ids = plot_hosts.get(pid).cloned().unwrap_or_default();
        let ys: Vec<f64> = ch_ids
            .iter()
            .filter_map(|cid| by_id.get(cid).map(|&i| tree.nodes[i].position.y))
            .collect();
        let mid_y = if ys.is_empty() {
            tree.nodes[*by_id.get(pid).unwrap()].position.y
        } else {
            (ys.iter().copied().fold(f64::INFINITY, f64::min)
                + ys.iter().copied().fold(f64::NEG_INFINITY, f64::max))
                / 2.0
        };
        let mut rightmost = f64::NEG_INFINITY;
        for cid in &ch_ids {
            if let Some(plots) = plots_by_host.get(cid) {
                for ex in plots {
                    rightmost = rightmost.max(tree.nodes[*by_id.get(ex).unwrap()].position.x);
                }
            }
        }
        let base_x = if rightmost == f64::NEG_INFINITY {
            PLOT_X
        } else {
            rightmost + PLOT_CARD_W + PLOT_CARD_W / 2.0
        };
        let mut x = base_x;
        for step in 0..20 {
            let cx = base_x + step as f64 * PLOT_DX;
            if !occupied.iter().any(|(ox, oy)| plot_overlaps(cx, mid_y, *ox, *oy)) {
                x = cx;
                break;
            }
        }
        let i = *by_id.get(pid).unwrap();
        tree.nodes[i].position = NodePosition { x, y: mid_y };
        occupied.push((x, mid_y));
    }

    orphan_plots.sort_by(|a, b| {
        by_y(
            &tree.nodes[*by_id.get(a).unwrap()],
            &tree.nodes[*by_id.get(b).unwrap()],
        )
    });
    place_grid_ids(
        &mut tree.nodes,
        &by_id,
        &orphan_plots,
        PLOT_X,
        y,
        PLOT_DX,
        PLOT_DY,
        MAX_PLOT_COL,
        1.0,
    );
    if !orphan_plots.is_empty() {
        y += grid_span(orphan_plots.len(), PLOT_DY, MAX_PLOT_COL) + CH_PAD;
    }
    orphan_chars.sort_by(|a, b| {
        by_y(
            &tree.nodes[*by_id.get(a).unwrap()],
            &tree.nodes[*by_id.get(b).unwrap()],
        )
    });
    place_side_ids(
        &mut tree.nodes,
        &by_id,
        &orphan_chars,
        CHAR_X,
        y,
        SIDE_DX,
        MAX_SIDE_COL,
        -1.0,
    );
    orphan_knows.sort_by(|a, b| {
        by_y(
            &tree.nodes[*by_id.get(a).unwrap()],
            &tree.nodes[*by_id.get(b).unwrap()],
        )
    });
    place_know_ids(&mut tree.nodes, &by_id, &orphan_knows, &orphan_chars, y);
}

fn dummy(id: &str, kind: NodeKind, y: f64) -> TreeNode {
    TreeNode {
        id: id.into(),
        kind,
        label: id.into(),
        outline: String::new(),
        character: None,
        knowledge: None,
        side_plot: None,
        linked_character_ids: vec![],
        linked_side_plot_ids: vec![],
        linked_knowledge_ids: vec![],
        position: NodePosition { x: 0.0, y },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(tree: &NovelTree, id: &str) -> NodePosition {
        tree.nodes.iter().find(|n| n.id == id).unwrap().position
    }

    #[test]
    fn layout_spine_and_linked_cards() {
        let mut r = dummy("r", NodeKind::Novel, 0.0);
        r.linked_character_ids = vec!["a_root".into()];
        r.linked_knowledge_ids = vec!["k_root".into()];
        let mut c1 = dummy("c1", NodeKind::Chapter, 10.0);
        c1.linked_side_plot_ids = vec!["p1".into(), "p2".into(), "p3".into(), "p4".into(), "p5".into(), "pm".into()];
        c1.linked_character_ids = vec!["a1".into(), "a2".into(), "a3".into(), "a4".into(), "a5".into()];
        c1.linked_knowledge_ids = vec!["k1".into()];
        let mut c2 = dummy("c2", NodeKind::Chapter, 20.0);
        c2.linked_side_plot_ids = vec!["pm".into()];
        let mut nodes = vec![r, c1, c2];
        for i in 1..=5 {
            nodes.push(dummy(&format!("p{i}"), NodeKind::SidePlot, i as f64));
        }
        nodes.push(dummy("pm", NodeKind::SidePlot, 0.0));
        for i in 1..=5 {
            nodes.push(dummy(&format!("a{i}"), NodeKind::Character, i as f64));
        }
        nodes.push(dummy("a_root", NodeKind::Character, 0.0));
        nodes.push(dummy("k_root", NodeKind::Knowledge, 0.0));
        nodes.push(dummy("k1", NodeKind::Knowledge, 99.0));
        let mut tree = NovelTree {
            novel_id: "t".into(),
            nodes,
            edges: vec![],
        };
        apply_auto_layout(&mut tree);
        assert_eq!(pos(&tree, "r").x, MAIN_X);
        assert_eq!(pos(&tree, "r").y, TOP);
        assert_eq!(pos(&tree, "a1").x, CHAR_X);
        assert_eq!(pos(&tree, "c1").x, MAIN_X);
        assert_eq!(pos(&tree, "p1").x, PLOT_X);
        assert_eq!(pos(&tree, "p2").y - pos(&tree, "p1").y, PLOT_DY);
        assert_eq!(pos(&tree, "p5").x, PLOT_X + PLOT_DX);
        assert_eq!(pos(&tree, "p5").y, pos(&tree, "p1").y);
        assert_eq!(pos(&tree, "a5").x, CHAR_X - SIDE_DX);
        assert!(pos(&tree, "a2").y > pos(&tree, "a1").y);
        let left_char = ["a1", "a2", "a3", "a4", "a5"]
            .iter()
            .map(|id| pos(&tree, id).x)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(pos(&tree, "k1").x, left_char - KNOW_GAP_FROM_CHAR);
        assert_eq!(pos(&tree, "k1").y, pos(&tree, "c1").y);
        assert_eq!(pos(&tree, "a_root").y, pos(&tree, "r").y);
        assert!(pos(&tree, "c1").y - pos(&tree, "r").y >= MIN_CHAPTER_GAP);
        let mid = (pos(&tree, "c1").y + pos(&tree, "c2").y) / 2.0;
        assert!((pos(&tree, "pm").y - mid).abs() < 1e-6);
        let rightmost = pos(&tree, "p1").x.max(pos(&tree, "p5").x);
        assert!((pos(&tree, "pm").x - (rightmost + PLOT_CARD_W + PLOT_CARD_W / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn layout_bare_chapters_tight() {
        let mut tree = NovelTree {
            novel_id: "t".into(),
            nodes: vec![
                dummy("br", NodeKind::Novel, 0.0),
                dummy("b1", NodeKind::Chapter, 10.0),
                dummy("b2", NodeKind::Chapter, 20.0),
                dummy("b3", NodeKind::Chapter, 30.0),
            ],
            edges: vec![],
        };
        apply_auto_layout(&mut tree);
        let d12 = pos(&tree, "b2").y - pos(&tree, "b1").y;
        assert!(d12 < MIN_CHAPTER_GAP);
        assert!(d12 >= MIN_CHAPTER_GAP_TIGHT);
        assert_eq!(d12, pos(&tree, "b3").y - pos(&tree, "b2").y);
    }

    #[test]
    fn layout_plot_order_follows_linked_ids() {
        let mut oc = dummy("oc", NodeKind::Chapter, 0.0);
        oc.linked_side_plot_ids = vec!["ob".into(), "oa".into()];
        let mut tree = NovelTree {
            novel_id: "t".into(),
            nodes: vec![
                oc,
                dummy("oa", NodeKind::SidePlot, 0.0),
                dummy("ob", NodeKind::SidePlot, 50.0),
            ],
            edges: vec![],
        };
        apply_auto_layout(&mut tree);
        assert_eq!(pos(&tree, "ob").y, pos(&tree, "oc").y);
        assert_eq!(pos(&tree, "oa").y, pos(&tree, "ob").y + PLOT_DY);
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
    fn layout_volumes_group_chapters() {
        // v2 初始 y 更小，但挂在 v1 之后；章应落在所属卷下方
        let mut tree = NovelTree {
            novel_id: "t".into(),
            nodes: vec![
                dummy("r", NodeKind::Novel, 0.0),
                dummy("v2", NodeKind::Volume, 1.0),
                dummy("v1", NodeKind::Volume, 50.0),
                dummy("cb", NodeKind::Chapter, 2.0),
                dummy("ca", NodeKind::Chapter, 3.0),
                dummy("cc", NodeKind::Chapter, 4.0),
            ],
            edges: vec![
                edge("e1", "r", "v1", "volume"),
                edge("e2", "v1", "v2", "volume"),
                edge("e3", "v1", "ca", "chapter"),
                edge("e4", "ca", "cb", "chapter"),
                edge("e5", "v2", "cc", "chapter"),
            ],
        };
        apply_auto_layout(&mut tree);
        assert!(pos(&tree, "r").y < pos(&tree, "v1").y);
        assert!(pos(&tree, "v1").y < pos(&tree, "ca").y);
        assert!(pos(&tree, "ca").y < pos(&tree, "cb").y);
        assert!(pos(&tree, "cb").y < pos(&tree, "v2").y);
        assert!(pos(&tree, "v2").y < pos(&tree, "cc").y);
        assert!(pos(&tree, "v1").y < pos(&tree, "v2").y);
    }
}
