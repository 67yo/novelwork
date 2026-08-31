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
const WV_FAN_R: f64 = 280.0;
const AXIOM_FAN_R: f64 = 200.0;
const LOCATION_FAN_R: f64 = 200.0;
const SOCIAL_EXT_FAN_R: f64 = 200.0;
const WV_FAN_SPAN_DEG: f64 = 110.0;
const EXT_FAN_SPAN_DEG: f64 = 56.0;
const MIN_EXT_FAN_SPAN_DEG: f64 = 28.0;
const WV_FIX_MAX_PASSES: usize = 12;
const WV_CARD_GAP: f64 = 16.0;
const MAX_FAN_R: f64 = 520.0;
const MAX_FAN_SPAN_DEG: f64 = 168.0;
const SOCIAL_HALF_GAP_DEG: f64 = 24.0;
const ROOT_CARD_W: f64 = 200.0;

const WV_FAN_ORDER: &[&str] = &[
    "wv_core_laws",
    "wv_spatiotemporal",
    "wv_social_power",
    "wv_history_culture",
    "wv_existence",
    "wv_info_flow",
];

fn knowledge_slot(n: &TreeNode) -> &str {
    n.knowledge
        .as_ref()
        .map(|k| k.slot.as_str())
        .unwrap_or("")
        .trim()
}

fn is_fan_slot(slot: &str) -> bool {
    WV_FAN_ORDER.contains(&slot)
}

fn is_worldview_layout_slot(slot: &str) -> bool {
    is_fan_slot(slot)
        || matches!(
            slot,
            "wv_axiom" | "wv_location" | "wv_race" | "wv_faction" | "wv_religion" | "wv_major_event"
        )
}

struct FanGeo {
    radius: f64,
    span_deg: f64,
}

fn resolve_fan_geometry(count: usize, base_radius: f64, base_span_deg: f64) -> FanGeo {
    if count <= 1 {
        return FanGeo {
            radius: base_radius,
            span_deg: 0.0,
        };
    }
    let min_arc = SIDE_CARD_W + WV_CARD_GAP;
    let mut span_deg = base_span_deg;
    let mut radius = base_radius.max(((count - 1) as f64 * min_arc * 180.0) / (std::f64::consts::PI * span_deg));
    if radius > MAX_FAN_R {
        radius = MAX_FAN_R;
        span_deg = ((count - 1) as f64 * min_arc * 180.0) / (std::f64::consts::PI * radius);
    }
    span_deg = span_deg.min(MAX_FAN_SPAN_DEG).max(base_span_deg);
    radius = radius.max(((count - 1) as f64 * min_arc * 180.0) / (std::f64::consts::PI * span_deg));
    FanGeo { radius, span_deg }
}

fn fan_rects_overlap(ax: f64, ay: f64, aw: f64, ah: f64, bx: f64, by: f64, bw: f64, bh: f64) -> bool {
    let pad = WV_CARD_GAP / 2.0;
    !(ax + aw + pad <= bx || bx + bw + pad <= ax || ay + ah + pad <= by || by + bh + pad <= ay)
}

fn place_half_fan(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_ids: &[String],
    anchor_idx: usize,
    base_radius: f64,
    deg_min: f64,
    deg_max: f64,
) {
    let n = fan_ids.len();
    if n == 0 {
        return;
    }
    let span = (deg_max - deg_min).max(1.0);
    let geo = resolve_fan_geometry(n, base_radius, span);
    let anchor = &nodes[anchor_idx];
    let cx = anchor.position.x + ROOT_CARD_W / 2.0;
    let cy = anchor.position.y;
    for (i, id) in fan_ids.iter().enumerate() {
        let deg = if n == 1 {
            (deg_min + deg_max) / 2.0
        } else {
            deg_min + ((deg_max - deg_min) * i as f64) / (n as f64 - 1.0)
        };
        let rad = deg.to_radians();
        let idx = *by_id.get(id).unwrap();
        let h = default_node_h(&nodes[idx].kind);
        nodes[idx].position = NodePosition {
            x: cx + geo.radius * rad.sin() - SIDE_CARD_W / 2.0,
            y: cy - geo.radius * rad.cos() - h,
        };
    }
}

fn place_fan_above(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_ids: &[String],
    anchor_idx: usize,
    base_radius: f64,
    span_deg: f64,
) -> f64 {
    let n = fan_ids.len();
    if n == 0 {
        return base_radius;
    }
    let geo = resolve_fan_geometry(n, base_radius, span_deg);
    let anchor = &nodes[anchor_idx];
    let cx = anchor.position.x + ROOT_CARD_W / 2.0;
    let cy = anchor.position.y;
    let start = -geo.span_deg / 2.0;
    for (i, id) in fan_ids.iter().enumerate() {
        let deg = if n == 1 {
            0.0
        } else {
            start + (geo.span_deg * i as f64) / (n as f64 - 1.0)
        };
        let rad = deg.to_radians();
        let idx = *by_id.get(id).unwrap();
        let h = default_node_h(&nodes[idx].kind);
        nodes[idx].position = NodePosition {
            x: cx + geo.radius * rad.sin() - SIDE_CARD_W / 2.0,
            y: cy - geo.radius * rad.cos() - h,
        };
    }
    geo.radius
}

fn place_worldview_fan(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_ids: &[String],
    root_idx: usize,
) -> f64 {
    place_fan_above(
        nodes,
        by_id,
        fan_ids,
        root_idx,
        WV_FAN_R,
        WV_FAN_SPAN_DEG,
    )
}

struct WvLayoutRadii {
    main_r: f64,
    main_span_deg: f64,
    axiom_r: f64,
    location_r: f64,
    social_r: f64,
    ext_span_deg: f64,
}

impl Default for WvLayoutRadii {
    fn default() -> Self {
        Self {
            main_r: WV_FAN_R,
            main_span_deg: WV_FAN_SPAN_DEG,
            axiom_r: AXIOM_FAN_R,
            location_r: LOCATION_FAN_R,
            social_r: SOCIAL_EXT_FAN_R,
            ext_span_deg: EXT_FAN_SPAN_DEG,
        }
    }
}

fn fan_cards_count(fan_by_slot: &HashMap<String, String>) -> usize {
    WV_FAN_ORDER
        .iter()
        .filter(|&&slot| fan_by_slot.contains_key(slot))
        .count()
}

fn extension_span_cap(
    child_count: usize,
    main_span_deg: f64,
    main_count: usize,
    base_span: f64,
) -> f64 {
    if child_count <= 1 {
        return 0.0;
    }
    let slot_deg = if main_count > 1 {
        main_span_deg / (main_count as f64 - 1.0)
    } else {
        main_span_deg
    };
    let max_span = base_span.min((slot_deg * 0.85 * (child_count as f64 - 1.0).max(1.0) + 8.0).max(MIN_EXT_FAN_SPAN_DEG));
    resolve_fan_geometry(child_count, AXIOM_FAN_R, max_span).span_deg
}

fn layout_worldview_extensions(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_by_slot: &HashMap<String, String>,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
    radii: &WvLayoutRadii,
) {
    let main_span = radii.main_span_deg;
    let main_count = fan_cards_count(fan_by_slot).max(1);
    let ext_base = radii.ext_span_deg;
    if let Some(core_id) = fan_by_slot.get("wv_core_laws") {
        if !axiom_ids.is_empty() {
            let core_idx = *by_id.get(core_id).unwrap();
            let order: HashMap<String, usize> = nodes[core_idx]
                .linked_knowledge_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (id.clone(), i))
                .collect();
            let mut sorted = axiom_ids.to_vec();
            sorted.sort_by(|a, b| {
                let ia = order.get(a).copied().unwrap_or(9999);
                let ib = order.get(b).copied().unwrap_or(9999);
                ia.cmp(&ib).then_with(|| {
                    by_y(
                        &nodes[*by_id.get(a).unwrap()],
                        &nodes[*by_id.get(b).unwrap()],
                    )
                })
            });
            place_fan_above(
                nodes,
                by_id,
                &sorted,
                core_idx,
                radii.axiom_r,
                extension_span_cap(sorted.len(), main_span, main_count, ext_base),
            );
        }
    }
    if let Some(st_id) = fan_by_slot.get("wv_spatiotemporal") {
        if !location_ids.is_empty() {
            let st_idx = *by_id.get(st_id).unwrap();
            let order: HashMap<String, usize> = nodes[st_idx]
                .linked_knowledge_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (id.clone(), i))
                .collect();
            let mut sorted = location_ids.to_vec();
            sorted.sort_by(|a, b| {
                let ia = order.get(a).copied().unwrap_or(9999);
                let ib = order.get(b).copied().unwrap_or(9999);
                ia.cmp(&ib).then_with(|| {
                    by_y(
                        &nodes[*by_id.get(a).unwrap()],
                        &nodes[*by_id.get(b).unwrap()],
                    )
                })
            });
            place_fan_above(
                nodes,
                by_id,
                &sorted,
                st_idx,
                radii.location_r,
                extension_span_cap(sorted.len(), main_span, main_count, ext_base),
            );
        }
    }
    if let Some(sp_id) = fan_by_slot.get("wv_social_power") {
        if !race_ids.is_empty() || !faction_ids.is_empty() {
            let sp_idx = *by_id.get(sp_id).unwrap();
            let order: HashMap<String, usize> = nodes[sp_idx]
                .linked_knowledge_ids
                .iter()
                .enumerate()
                .map(|(i, id)| (id.clone(), i))
                .collect();
            let mut sorted_race = race_ids.to_vec();
            sorted_race.sort_by(|a, b| {
                let ia = order.get(a).copied().unwrap_or(9999);
                let ib = order.get(b).copied().unwrap_or(9999);
                ia.cmp(&ib).then_with(|| {
                    by_y(
                        &nodes[*by_id.get(a).unwrap()],
                        &nodes[*by_id.get(b).unwrap()],
                    )
                })
            });
            let mut sorted_faction = faction_ids.to_vec();
            sorted_faction.sort_by(|a, b| {
                let ia = order.get(a).copied().unwrap_or(9999);
                let ib = order.get(b).copied().unwrap_or(9999);
                ia.cmp(&ib).then_with(|| {
                    by_y(
                        &nodes[*by_id.get(a).unwrap()],
                        &nodes[*by_id.get(b).unwrap()],
                    )
                })
            });
            let race_span = extension_span_cap(sorted_race.len(), main_span, main_count, ext_base);
            let fac_span = extension_span_cap(sorted_faction.len(), main_span, main_count, ext_base);
            let both = !sorted_race.is_empty() && !sorted_faction.is_empty();
            let full_span = race_span.max(fac_span).max(ext_base);
            let half = if both {
                (full_span - SOCIAL_HALF_GAP_DEG) / 2.0
            } else {
                full_span / 2.0
            };
            let gap_half = SOCIAL_HALF_GAP_DEG / 2.0;
            if !sorted_race.is_empty() {
                place_half_fan(
                    nodes,
                    by_id,
                    &sorted_race,
                    sp_idx,
                    radii.social_r,
                    if both { -full_span / 2.0 } else { -half },
                    if both { -gap_half } else { half },
                );
            }
            if !sorted_faction.is_empty() {
                place_half_fan(
                    nodes,
                    by_id,
                    &sorted_faction,
                    sp_idx,
                    radii.social_r,
                    if both { gap_half } else { -half },
                    if both { full_span / 2.0 } else { half },
                );
            }
        }
    }
}

fn worldview_has_overlap(nodes: &[TreeNode], wv_ids: &[String], by_id: &HashMap<String, usize>) -> bool {
    for i in 0..wv_ids.len() {
        for j in (i + 1)..wv_ids.len() {
            let ai = *by_id.get(&wv_ids[i]).unwrap();
            let bi = *by_id.get(&wv_ids[j]).unwrap();
            let ah = default_node_h(&nodes[ai].kind);
            let bh = default_node_h(&nodes[bi].kind);
            if fan_rects_overlap(
                nodes[ai].position.x,
                nodes[ai].position.y,
                SIDE_CARD_W,
                ah,
                nodes[bi].position.x,
                nodes[bi].position.y,
                SIDE_CARD_W,
                bh,
            ) {
                return true;
            }
        }
    }
    false
}

fn nudge_worldview_overlaps(nodes: &mut [TreeNode], wv_ids: &[String], by_id: &HashMap<String, usize>) {
    for _pass in 0..8 {
        let mut moved = false;
        for i in 0..wv_ids.len() {
            for j in (i + 1)..wv_ids.len() {
                let ai = *by_id.get(&wv_ids[i]).unwrap();
                let bi = *by_id.get(&wv_ids[j]).unwrap();
                let ah = default_node_h(&nodes[ai].kind);
                let bh = default_node_h(&nodes[bi].kind);
                if !fan_rects_overlap(
                    nodes[ai].position.x,
                    nodes[ai].position.y,
                    SIDE_CARD_W,
                    ah,
                    nodes[bi].position.x,
                    nodes[bi].position.y,
                    SIDE_CARD_W,
                    bh,
                ) {
                    continue;
                }
                let (upper, lower) = if nodes[ai].position.y <= nodes[bi].position.y {
                    (ai, bi)
                } else {
                    (bi, ai)
                };
                let uh = default_node_h(&nodes[upper].kind);
                let overlap_y = nodes[upper].position.y + uh - nodes[lower].position.y;
                if overlap_y > 0.0 {
                    nodes[upper].position.y -= overlap_y + WV_CARD_GAP;
                    moved = true;
                }
                let lh = default_node_h(&nodes[lower].kind);
                if fan_rects_overlap(
                    nodes[upper].position.x,
                    nodes[upper].position.y,
                    SIDE_CARD_W,
                    uh,
                    nodes[lower].position.x,
                    nodes[lower].position.y,
                    SIDE_CARD_W,
                    lh,
                ) {
                    let push_x = (nodes[upper].position.x + SIDE_CARD_W - nodes[lower].position.x)
                        / 2.0
                        + WV_CARD_GAP / 2.0;
                    if push_x > 0.0 && nodes[upper].position.x <= nodes[lower].position.x {
                        nodes[upper].position.x -= push_x;
                        nodes[lower].position.x += push_x;
                        moved = true;
                    }
                }
            }
        }
        if !moved {
            return;
        }
    }
}

fn relayout_worldview(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_ids: &[String],
    root_idx: usize,
    fan_by_slot: &HashMap<String, String>,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
    radii: &mut WvLayoutRadii,
) {
    let main_geo = resolve_fan_geometry(fan_ids.len(), radii.main_r, radii.main_span_deg);
    radii.main_r = main_geo.radius;
    radii.main_span_deg = main_geo.span_deg;
    place_fan_above(
        nodes,
        by_id,
        fan_ids,
        root_idx,
        radii.main_r,
        radii.main_span_deg,
    );
    layout_worldview_extensions(
        nodes,
        by_id,
        fan_by_slot,
        axiom_ids,
        location_ids,
        race_ids,
        faction_ids,
        radii,
    );
}

fn card_center_x(n: &TreeNode) -> f64 {
    n.position.x + SIDE_CARD_W / 2.0
}

fn reach_from_center(main: &TreeNode, kid_ids: &[String], by_id: &HashMap<String, usize>, nodes: &[TreeNode], dir: f64) -> f64 {
    let mc = card_center_x(main);
    let mut reach = SIDE_CARD_W / 2.0;
    for id in kid_ids {
        let Some(&ki) = by_id.get(id) else { continue };
        let k = &nodes[ki];
        if dir > 0.0 {
            reach = reach.max(k.position.x + SIDE_CARD_W - mc);
        } else {
            reach = reach.max(mc - k.position.x);
        }
    }
    reach
}

fn adjacent_mains_too_close(
    nodes: &[TreeNode],
    by_id: &HashMap<String, usize>,
    fan_by_slot: &HashMap<String, String>,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
) -> bool {
    let mut mains: Vec<&str> = WV_FAN_ORDER
        .iter()
        .filter_map(|s| fan_by_slot.get(*s).map(|id| id.as_str()))
        .collect();
    mains.sort_by(|a, b| {
        let ca = card_center_x(&nodes[*by_id.get(*a).unwrap()]);
        let cb = card_center_x(&nodes[*by_id.get(*b).unwrap()]);
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    let kids_of = |slot: &str| -> Vec<String> {
        match slot {
            "wv_core_laws" => axiom_ids.to_vec(),
            "wv_spatiotemporal" => location_ids.to_vec(),
            "wv_social_power" => {
                let mut v = race_ids.to_vec();
                v.extend(faction_ids.iter().cloned());
                v
            }
            _ => vec![],
        }
    };
    for i in 0..mains.len().saturating_sub(1) {
        let left_id = mains[i];
        let right_id = mains[i + 1];
        let li = *by_id.get(left_id).unwrap();
        let ri = *by_id.get(right_id).unwrap();
        let left = &nodes[li];
        let right = &nodes[ri];
        let left_slot = knowledge_slot(left);
        let right_slot = knowledge_slot(right);
        let need = reach_from_center(left, &kids_of(left_slot), by_id, nodes, 1.0)
            + reach_from_center(right, &kids_of(right_slot), by_id, nodes, -1.0)
            + WV_CARD_GAP;
        let have = card_center_x(right) - card_center_x(left);
        if have + 0.5 < need {
            return true;
        }
    }
    false
}

fn separate_mains_by_child_reach(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_by_slot: &HashMap<String, String>,
    root_idx: usize,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
) -> bool {
    let mut mains: Vec<String> = WV_FAN_ORDER
        .iter()
        .filter_map(|s| fan_by_slot.get(*s).cloned())
        .collect();
    if mains.len() < 2 {
        return false;
    }
    mains.sort_by(|a, b| {
        let ca = card_center_x(&nodes[*by_id.get(a).unwrap()]);
        let cb = card_center_x(&nodes[*by_id.get(b).unwrap()]);
        ca.partial_cmp(&cb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(b))
    });
    let kids_of = |slot: &str| -> Vec<String> {
        match slot {
            "wv_core_laws" => axiom_ids.to_vec(),
            "wv_spatiotemporal" => location_ids.to_vec(),
            "wv_social_power" => {
                let mut v = race_ids.to_vec();
                v.extend(faction_ids.iter().cloned());
                v
            }
            _ => vec![],
        }
    };
    let mut reach_r = Vec::with_capacity(mains.len());
    let mut reach_l = Vec::with_capacity(mains.len());
    for id in &mains {
        let i = *by_id.get(id).unwrap();
        let slot = knowledge_slot(&nodes[i]).to_string();
        let kids = kids_of(&slot);
        reach_r.push(reach_from_center(&nodes[i], &kids, by_id, nodes, 1.0));
        reach_l.push(reach_from_center(&nodes[i], &kids, by_id, nodes, -1.0));
    }
    let mut target_cx = vec![card_center_x(&nodes[*by_id.get(&mains[0]).unwrap()])];
    for i in 1..mains.len() {
        target_cx.push(target_cx[i - 1] + reach_r[i - 1] + reach_l[i] + WV_CARD_GAP);
    }
    let mid = (target_cx[0] + target_cx[target_cx.len() - 1]) / 2.0;
    let root_cx = nodes[root_idx].position.x + ROOT_CARD_W / 2.0;
    let shift = root_cx - mid;
    for c in &mut target_cx {
        *c += shift;
    }
    let mut changed = false;
    for (i, id) in mains.iter().enumerate() {
        let mi = *by_id.get(id).unwrap();
        let nx = target_cx[i] - SIDE_CARD_W / 2.0;
        if (nx - nodes[mi].position.x).abs() > 0.5 {
            nodes[mi].position.x = nx;
            changed = true;
        }
    }
    changed
}

fn place_worldview_children_only(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_by_slot: &HashMap<String, String>,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
    radii: &WvLayoutRadii,
) {
    layout_worldview_extensions(
        nodes,
        by_id,
        fan_by_slot,
        axiom_ids,
        location_ids,
        race_ids,
        faction_ids,
        radii,
    );
}

fn fix_worldview_overlaps(
    nodes: &mut [TreeNode],
    by_id: &HashMap<String, usize>,
    fan_ids: &[String],
    root_idx: usize,
    fan_by_slot: &HashMap<String, String>,
    axiom_ids: &[String],
    location_ids: &[String],
    race_ids: &[String],
    faction_ids: &[String],
) {
    let wv_ids: Vec<String> = nodes
        .iter()
        .filter(|n| {
            n.kind == NodeKind::Knowledge && is_worldview_layout_slot(knowledge_slot(n))
        })
        .map(|n| n.id.clone())
        .collect();
    if wv_ids.len() < 2 || fan_ids.is_empty() {
        return;
    }
    let mut radii = WvLayoutRadii::default();
    relayout_worldview(
        nodes,
        by_id,
        fan_ids,
        root_idx,
        fan_by_slot,
        axiom_ids,
        location_ids,
        race_ids,
        faction_ids,
        &mut radii,
    );

    for _pass in 0..WV_FIX_MAX_PASSES {
        separate_mains_by_child_reach(
            nodes,
            by_id,
            fan_by_slot,
            root_idx,
            axiom_ids,
            location_ids,
            race_ids,
            faction_ids,
        );
        place_worldview_children_only(
            nodes,
            by_id,
            fan_by_slot,
            axiom_ids,
            location_ids,
            race_ids,
            faction_ids,
            &radii,
        );
        let tight = adjacent_mains_too_close(
            nodes,
            by_id,
            fan_by_slot,
            axiom_ids,
            location_ids,
            race_ids,
            faction_ids,
        );
        let has_overlap = worldview_has_overlap(nodes, &wv_ids, by_id);
        if !tight && !has_overlap {
            break;
        }
        let mut bump_ax = false;
        let mut bump_loc = false;
        let mut bump_soc = false;
        for i in 0..wv_ids.len() {
            for j in (i + 1)..wv_ids.len() {
                let ai = *by_id.get(&wv_ids[i]).unwrap();
                let bi = *by_id.get(&wv_ids[j]).unwrap();
                let ah = default_node_h(&nodes[ai].kind);
                let bh = default_node_h(&nodes[bi].kind);
                if !fan_rects_overlap(
                    nodes[ai].position.x,
                    nodes[ai].position.y,
                    SIDE_CARD_W,
                    ah,
                    nodes[bi].position.x,
                    nodes[bi].position.y,
                    SIDE_CARD_W,
                    bh,
                ) {
                    continue;
                }
                let sa = knowledge_slot(&nodes[ai]);
                let sb = knowledge_slot(&nodes[bi]);
                if sa == "wv_axiom" && sb == "wv_axiom" {
                    bump_ax = true;
                }
                if sa == "wv_location" && sb == "wv_location" {
                    bump_loc = true;
                }
                if (sa == "wv_race" || sa == "wv_faction")
                    && (sb == "wv_race" || sb == "wv_faction")
                {
                    bump_soc = true;
                }
            }
        }
        if bump_ax {
            radii.axiom_r += 24.0;
        }
        if bump_loc {
            radii.location_r += 24.0;
        }
        if bump_soc {
            radii.social_r += 24.0;
        }
        if bump_ax || bump_loc || bump_soc {
            radii.ext_span_deg = (radii.ext_span_deg - 3.0).max(MIN_EXT_FAN_SPAN_DEG);
        } else if !tight {
            break;
        }
    }

    for _ in 0..2 {
        separate_mains_by_child_reach(
            nodes,
            by_id,
            fan_by_slot,
            root_idx,
            axiom_ids,
            location_ids,
            race_ids,
            faction_ids,
        );
        place_worldview_children_only(
            nodes,
            by_id,
            fan_by_slot,
            axiom_ids,
            location_ids,
            race_ids,
            faction_ids,
            &radii,
        );
    }
    nudge_worldview_overlaps(nodes, &wv_ids, by_id);
}

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
    let mut fan_by_slot: HashMap<String, String> = HashMap::new();
    let mut story_rules_id: Option<String> = None;
    let mut axiom_ids: Vec<String> = Vec::new();
    let mut location_ids: Vec<String> = Vec::new();
    let mut race_ids: Vec<String> = Vec::new();
    let mut faction_ids: Vec<String> = Vec::new();
    for n in &tree.nodes {
        match n.kind {
            NodeKind::Character => {
                if let Some(hid) = char_host.get(&n.id) {
                    if spine_set.contains(hid) {
                        chars_by_host
                            .entry(hid.clone())
                            .or_default()
                            .push(n.id.clone());
                        continue;
                    }
                }
                orphan_chars.push(n.id.clone());
            }
            NodeKind::Knowledge => {
                let slot = knowledge_slot(n).to_string();
                if slot == "story_rules" {
                    story_rules_id = Some(n.id.clone());
                    continue;
                }
                if slot == "wv_axiom" {
                    axiom_ids.push(n.id.clone());
                    continue;
                }
                if slot == "wv_location" {
                    location_ids.push(n.id.clone());
                    continue;
                }
                if slot == "wv_race" {
                    race_ids.push(n.id.clone());
                    continue;
                }
                if slot == "wv_faction" {
                    faction_ids.push(n.id.clone());
                    continue;
                }
                if is_fan_slot(&slot) {
                    fan_by_slot.insert(slot, n.id.clone());
                    continue;
                }
                if let Some(hid) = know_host.get(&n.id) {
                    if spine_set.contains(hid) {
                        knows_by_host
                            .entry(hid.clone())
                            .or_default()
                            .push(n.id.clone());
                        continue;
                    }
                }
                orphan_knows.push(n.id.clone());
            }
            _ => {}
        }
    }
    let fan_ids: Vec<String> = WV_FAN_ORDER
        .iter()
        .filter_map(|s| fan_by_slot.get(*s).cloned())
        .collect();
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
    if !fan_ids.is_empty() {
        let geo = resolve_fan_geometry(fan_ids.len(), WV_FAN_R, WV_FAN_SPAN_DEG);
        y += geo.radius + default_node_h(&NodeKind::Knowledge) + SIDE_GAP;
    }
    if !axiom_ids.is_empty() {
        let geo = resolve_fan_geometry(axiom_ids.len(), AXIOM_FAN_R, EXT_FAN_SPAN_DEG);
        y += geo.radius + default_node_h(&NodeKind::Knowledge) + SIDE_GAP;
    }
    if !location_ids.is_empty() {
        let geo = resolve_fan_geometry(location_ids.len(), LOCATION_FAN_R, EXT_FAN_SPAN_DEG);
        y += geo.radius + default_node_h(&NodeKind::Knowledge) + SIDE_GAP;
    }
    if !race_ids.is_empty() || !faction_ids.is_empty() {
        let rr = if race_ids.is_empty() {
            0.0
        } else {
            resolve_fan_geometry(race_ids.len(), SOCIAL_EXT_FAN_R, EXT_FAN_SPAN_DEG).radius
        };
        let fr = if faction_ids.is_empty() {
            0.0
        } else {
            resolve_fan_geometry(faction_ids.len(), SOCIAL_EXT_FAN_R, EXT_FAN_SPAN_DEG).radius
        };
        y += rr.max(fr) + default_node_h(&NodeKind::Knowledge) + SIDE_GAP;
    }
    for hid in &spine_ids {
        let hi = *by_id.get(hid).unwrap();
        tree.nodes[hi].position = NodePosition { x: MAIN_X, y };
        if let Some(plots) = plots_by_host.get_mut(hid) {
            sort_plots_for_host(&tree.nodes[hi], plots, &by_id, &tree.nodes);
        }
        let plot_ids = plots_by_host.get(hid).cloned().unwrap_or_default();
        let char_ids = chars_by_host.get(hid).cloned().unwrap_or_default();
        let know_ids = knows_by_host.get(hid).cloned().unwrap_or_default();

        let is_root = tree.nodes[hi].kind == NodeKind::Novel;
        if is_root {
            fix_worldview_overlaps(
                &mut tree.nodes,
                &by_id,
                &fan_ids,
                hi,
                &fan_by_slot,
                &axiom_ids,
                &location_ids,
                &race_ids,
                &faction_ids,
            );
            if let Some(ref sid) = story_rules_id {
                let si = *by_id.get(sid).unwrap();
                tree.nodes[si].position = NodePosition {
                    x: PLOT_X,
                    y: tree.nodes[hi].position.y,
                };
                let p = &tree.nodes[si].position;
                occupied.push((p.x, p.y));
            }
        }

        let plot_origin_x = if is_root && story_rules_id.is_some() {
            PLOT_X + PLOT_DX
        } else {
            PLOT_X
        };
        place_grid_ids(
            &mut tree.nodes,
            &by_id,
            &plot_ids,
            plot_origin_x,
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
        let story_span = if is_root && story_rules_id.is_some() {
            PLOT_DY
        } else {
            0.0
        };
        let span = grid_span(plot_ids.len(), PLOT_DY, MAX_PLOT_COL)
            .max(char_span)
            .max(know_span)
            .max(story_span);
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
        detailed_outline: vec![],
        character: None,
        knowledge: None,
        side_plot: None,
        volume: None,
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
