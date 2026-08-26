import type { NovelTree, TreeNode } from "@/lib/api";
import {
  LOCATION_SLOT,
  emptyLocation,
  formatKeyLocationExtracted,
  formatSpatiotemporalExtracted,
  isLocationSlot,
  locationHasContent,
  normalizeLocation,
  normalizeSpatiotemporal,
  type KeyLocation,
} from "@/lib/spatiotemporal";

/** 时空地理节点上挂着的关键地点子卡 */
export function linkedLocationCards(tree: NovelTree, spatiotemporalId: string): TreeNode[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const ids = new Set<string>();
  const host = byId.get(spatiotemporalId);
  for (const id of host?.linked_knowledge_ids ?? []) ids.add(id);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other =
      e.source === spatiotemporalId
        ? e.target
        : e.target === spatiotemporalId
          ? e.source
          : null;
    if (other) ids.add(other);
  }
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n?.kind === "knowledge" && isLocationSlot(n.knowledge?.slot)) out.push(n);
  }
  out.sort(
    (a, b) =>
      a.position.x - b.position.x ||
      a.position.y - b.position.y ||
      a.id.localeCompare(b.id),
  );
  return out;
}

export function locationsFromLinkedCards(tree: NovelTree, spatiotemporalId: string): KeyLocation[] {
  return linkedLocationCards(tree, spatiotemporalId).map((n) =>
    normalizeLocation(n.knowledge?.key_location ?? emptyLocation()),
  );
}

/** 用子卡内容重写时空地理 extracted（并清空内嵌 locations） */
export function syncSpatiotemporalExtracted(tree: NovelTree, spatiotemporalId: string): void {
  const host = tree.nodes.find((n) => n.id === spatiotemporalId && n.kind === "knowledge");
  if (!host?.knowledge) return;
  const st = normalizeSpatiotemporal(host.knowledge.spatiotemporal);
  st.locations = [];
  host.knowledge.spatiotemporal = st;
  const extracted = formatSpatiotemporalExtracted(
    st,
    locationsFromLinkedCards(tree, spatiotemporalId),
  );
  host.knowledge.extracted = extracted;
  host.outline = extracted ? [...extracted].slice(0, 200).join("") : "";
}

/** 把 spatiotemporal.locations 迁到扇形子卡；返回是否改树 */
export function migrateInlineLocationsToCards(tree: NovelTree): boolean {
  const host = tree.nodes.find(
    (n) => n.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === "wv_spatiotemporal",
  );
  if (!host?.knowledge?.spatiotemporal) return false;
  const st = normalizeSpatiotemporal(host.knowledge.spatiotemporal);
  const pending = st.locations.filter(locationHasContent);
  if (!pending.length) {
    if (st.locations.length) {
      st.locations = [];
      host.knowledge.spatiotemporal = st;
      syncSpatiotemporalExtracted(tree, host.id);
      return true;
    }
    return false;
  }
  if (linkedLocationCards(tree, host.id).length) {
    st.locations = [];
    host.knowledge.spatiotemporal = st;
    syncSpatiotemporalExtracted(tree, host.id);
    return true;
  }
  host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
  for (const loc of pending) {
    const id = crypto.randomUUID();
    const label = loc.name.trim() || "关键地点";
    const extracted = formatKeyLocationExtracted(loc);
    tree.nodes.push({
      id,
      kind: "knowledge",
      label,
      outline: extracted.slice(0, 200),
      character: null,
      knowledge: {
        book_ids: [],
        extract_prompt: "",
        extracted,
        slot: LOCATION_SLOT,
        key_location: normalizeLocation(loc),
      },
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { ...host.position },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    });
    host.linked_knowledge_ids.push(id);
    tree.edges.push({
      id: `e-${host.id}-${id}`,
      source: host.id,
      target: id,
      kind: "knowledge",
      source_handle: "top",
      target_handle: "bottom",
    });
  }
  st.locations = [];
  host.knowledge.spatiotemporal = st;
  syncSpatiotemporalExtracted(tree, host.id);
  return true;
}

export function createLocationCard(
  tree: NovelTree,
  spatiotemporalId: string,
  label: string,
  loc?: KeyLocation,
): TreeNode | null {
  const host = tree.nodes.find((n) => n.id === spatiotemporalId && n.kind === "knowledge");
  if (!host) return null;
  const id = crypto.randomUUID();
  const keyLocation = normalizeLocation(loc ?? { ...emptyLocation(), name: label });
  const node: TreeNode = {
    id,
    kind: "knowledge",
    label,
    outline: "",
    character: null,
    knowledge: {
      book_ids: [],
      extract_prompt: "",
      extracted: "",
      slot: LOCATION_SLOT,
      key_location: keyLocation,
    },
    side_plot: null,
    linked_character_ids: [],
    linked_side_plot_ids: [],
    linked_knowledge_ids: [],
    position: { ...host.position },
    word_count: 0,
    word_count_min: 0,
    word_count_max: 0,
    chapter_count: 0,
  };
  tree.nodes.push(node);
  host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
  if (!host.linked_knowledge_ids.includes(id)) host.linked_knowledge_ids.push(id);
  tree.edges.push({
    id: `e-${host.id}-${id}`,
    source: host.id,
    target: id,
    kind: "knowledge",
    source_handle: "top",
    target_handle: "bottom",
  });
  syncSpatiotemporalExtracted(tree, spatiotemporalId);
  return node;
}

/** 地点卡宿主：时空地理节点 id */
export function locationHostSpatiotemporalId(tree: NovelTree, locationId: string): string | null {
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === locationId ? e.target : e.target === locationId ? e.source : null;
    if (!other) continue;
    const n = tree.nodes.find((x) => x.id === other);
    if (n?.kind === "knowledge" && (n.knowledge?.slot ?? "") === "wv_spatiotemporal") return other;
  }
  for (const n of tree.nodes) {
    if (
      n.kind === "knowledge" &&
      (n.knowledge?.slot ?? "") === "wv_spatiotemporal" &&
      (n.linked_knowledge_ids ?? []).includes(locationId)
    ) {
      return n.id;
    }
  }
  return null;
}
