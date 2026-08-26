import type { NovelTree, TreeNode } from "@/lib/api";
import {
  MAJOR_EVENT_SLOT,
  RELIGION_SLOT,
  emptyMajorEvent,
  emptyReligion,
  formatHistoryCultureExtracted,
  formatMajorEventExtracted,
  formatWorldReligionExtracted,
  isMajorEventSlot,
  isReligionSlot,
  majorEventHasContent,
  normalizeHistoryCulture,
  normalizeMajorEvent,
  normalizeReligion,
  religionHasContent,
  type MajorEvent,
  type WorldReligion,
} from "@/lib/historyCulture";

function linkedBySlot(tree: NovelTree, hostId: string, slot: string): TreeNode[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const ids = new Set<string>();
  const host = byId.get(hostId);
  for (const id of host?.linked_knowledge_ids ?? []) ids.add(id);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other =
      e.source === hostId ? e.target : e.target === hostId ? e.source : null;
    if (other) ids.add(other);
  }
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n?.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === slot) out.push(n);
  }
  out.sort(
    (a, b) =>
      a.position.x - b.position.x ||
      a.position.y - b.position.y ||
      a.id.localeCompare(b.id),
  );
  return out;
}

export function linkedReligionCards(tree: NovelTree, hostId: string): TreeNode[] {
  return linkedBySlot(tree, hostId, RELIGION_SLOT);
}

export function linkedMajorEventCards(tree: NovelTree, hostId: string): TreeNode[] {
  return linkedBySlot(tree, hostId, MAJOR_EVENT_SLOT);
}

export function religionsFromLinkedCards(tree: NovelTree, hostId: string): WorldReligion[] {
  return linkedReligionCards(tree, hostId).map((n) =>
    normalizeReligion(n.knowledge?.world_religion ?? emptyReligion()),
  );
}

export function majorEventsFromLinkedCards(tree: NovelTree, hostId: string): MajorEvent[] {
  return linkedMajorEventCards(tree, hostId).map((n) =>
    normalizeMajorEvent(n.knowledge?.major_event ?? emptyMajorEvent()),
  );
}

export function syncHistoryCultureExtracted(tree: NovelTree, hostId: string): void {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host?.knowledge) return;
  const hc = normalizeHistoryCulture(host.knowledge.history_culture);
  hc.religions = [];
  hc.major_events = [];
  host.knowledge.history_culture = hc;
  const extracted = formatHistoryCultureExtracted(
    hc,
    religionsFromLinkedCards(tree, hostId),
    majorEventsFromLinkedCards(tree, hostId),
  );
  host.knowledge.extracted = extracted;
  host.outline = extracted ? [...extracted].slice(0, 200).join("") : "";
}

export function migrateInlineHistoryCultureToCards(tree: NovelTree): boolean {
  const host = tree.nodes.find(
    (n) => n.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === "wv_history_culture",
  );
  if (!host?.knowledge?.history_culture) return false;
  const hc = normalizeHistoryCulture(host.knowledge.history_culture);
  const pendingRel = hc.religions.filter(religionHasContent);
  const pendingEv = hc.major_events.filter(majorEventHasContent);
  if (!pendingRel.length && !pendingEv.length) {
    if (hc.religions.length || hc.major_events.length) {
      hc.religions = [];
      hc.major_events = [];
      host.knowledge.history_culture = hc;
      syncHistoryCultureExtracted(tree, host.id);
      return true;
    }
    return false;
  }
  if (linkedReligionCards(tree, host.id).length || linkedMajorEventCards(tree, host.id).length) {
    hc.religions = [];
    hc.major_events = [];
    host.knowledge.history_culture = hc;
    syncHistoryCultureExtracted(tree, host.id);
    return true;
  }
  host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
  for (const rel of pendingRel) {
    const id = crypto.randomUUID();
    const label = rel.name.trim() || "宗教";
    const extracted = formatWorldReligionExtracted(rel);
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
        slot: RELIGION_SLOT,
        world_religion: normalizeReligion(rel),
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
  for (const ev of pendingEv) {
    const id = crypto.randomUUID();
    const label = ev.title.trim() || "重大事件";
    const extracted = formatMajorEventExtracted(ev);
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
        slot: MAJOR_EVENT_SLOT,
        major_event: normalizeMajorEvent(ev),
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
  hc.religions = [];
  hc.major_events = [];
  host.knowledge.history_culture = hc;
  syncHistoryCultureExtracted(tree, host.id);
  return true;
}

function pushChildCard(
  tree: NovelTree,
  host: TreeNode,
  _slot: string,
  label: string,
  knowledge: NonNullable<TreeNode["knowledge"]>,
): TreeNode {
  const id = crypto.randomUUID();
  const node: TreeNode = {
    id,
    kind: "knowledge",
    label,
    outline: "",
    character: null,
    knowledge,
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
  return node;
}

export function createReligionCard(tree: NovelTree, hostId: string, label: string): TreeNode | null {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host) return null;
  const rel = emptyReligion();
  rel.name = label;
  const node = pushChildCard(tree, host, RELIGION_SLOT, label, {
    book_ids: [],
    extract_prompt: "",
    extracted: "",
    slot: RELIGION_SLOT,
    world_religion: rel,
  });
  syncHistoryCultureExtracted(tree, hostId);
  return node;
}

export function createMajorEventCard(
  tree: NovelTree,
  hostId: string,
  label: string,
): TreeNode | null {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host) return null;
  const ev = emptyMajorEvent();
  ev.title = label;
  const node = pushChildCard(tree, host, MAJOR_EVENT_SLOT, label, {
    book_ids: [],
    extract_prompt: "",
    extracted: "",
    slot: MAJOR_EVENT_SLOT,
    major_event: ev,
  });
  syncHistoryCultureExtracted(tree, hostId);
  return node;
}

export function historyCultureHostId(tree: NovelTree, childId: string): string | null {
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === childId ? e.target : e.target === childId ? e.source : null;
    if (!other) continue;
    const n = tree.nodes.find((x) => x.id === other);
    if (n?.kind === "knowledge" && (n.knowledge?.slot ?? "") === "wv_history_culture") return other;
  }
  for (const n of tree.nodes) {
    if (
      n.kind === "knowledge" &&
      (n.knowledge?.slot ?? "") === "wv_history_culture" &&
      (n.linked_knowledge_ids ?? []).includes(childId)
    ) {
      return n.id;
    }
  }
  return null;
}

export function isHistoryCultureChildSlot(slot: string | undefined | null): boolean {
  return isReligionSlot(slot) || isMajorEventSlot(slot);
}
