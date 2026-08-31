import type { NovelTree, TreeNode } from "@/lib/api";
import {
  FACTION_SLOT,
  RACE_SLOT,
  emptyFaction,
  emptyRace,
  factionHasContent,
  formatMajorFactionExtracted,
  formatSocialPowerExtracted,
  formatWorldRaceExtracted,
  isFactionSlot,
  isRaceSlot,
  normalizeFaction,
  normalizeRace,
  normalizeSocialPower,
  raceHasContent,
  type MajorFaction,
  type WorldRace,
} from "@/lib/socialPower";

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

export function linkedRaceCards(tree: NovelTree, hostId: string): TreeNode[] {
  return linkedBySlot(tree, hostId, RACE_SLOT);
}

export function linkedFactionCards(tree: NovelTree, hostId: string): TreeNode[] {
  return linkedBySlot(tree, hostId, FACTION_SLOT);
}

export function racesFromLinkedCards(tree: NovelTree, hostId: string): WorldRace[] {
  return linkedRaceCards(tree, hostId).map((n) =>
    normalizeRace(n.knowledge?.world_race ?? emptyRace()),
  );
}

export function factionsFromLinkedCards(tree: NovelTree, hostId: string): MajorFaction[] {
  return linkedFactionCards(tree, hostId).map((n) =>
    normalizeFaction(n.knowledge?.major_faction ?? emptyFaction()),
  );
}

export function syncSocialPowerExtracted(tree: NovelTree, hostId: string): void {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host?.knowledge) return;
  const sp = normalizeSocialPower(host.knowledge.social_power);
  sp.races = [];
  sp.factions = [];
  host.knowledge.social_power = sp;
  const extracted = formatSocialPowerExtracted(
    sp,
    racesFromLinkedCards(tree, hostId),
    factionsFromLinkedCards(tree, hostId),
  );
  host.knowledge.extracted = extracted;
  host.outline = extracted ? [...extracted].slice(0, 200).join("") : "";
}

function pushChildCard(
  tree: NovelTree,
  host: TreeNode,
  slot: string,
  label: string,
  extracted: string,
  payload: Record<string, unknown>,
): string {
  const id = crypto.randomUUID();
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
      slot,
      ...payload,
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
  host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
  host.linked_knowledge_ids.push(id);
  tree.edges.push({
    id: `e-${host.id}-${id}`,
    source: host.id,
    target: id,
    kind: "knowledge",
    source_handle: "top",
    target_handle: "bottom",
  });
  return id;
}

export function migrateInlineSocialPowerToCards(tree: NovelTree): boolean {
  const host = tree.nodes.find(
    (n) => n.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === "wv_social_power",
  );
  if (!host?.knowledge?.social_power) return false;
  const sp = normalizeSocialPower(host.knowledge.social_power);
  const pendingRaces = sp.races.filter(raceHasContent);
  const pendingFactions = sp.factions.filter(factionHasContent);
  const hasChild =
    linkedRaceCards(tree, host.id).length > 0 ||
    linkedFactionCards(tree, host.id).length > 0;
  if (!pendingRaces.length && !pendingFactions.length) {
    if (sp.races.length || sp.factions.length) {
      sp.races = [];
      sp.factions = [];
      host.knowledge.social_power = sp;
      syncSocialPowerExtracted(tree, host.id);
      return true;
    }
    return false;
  }
  if (hasChild) {
    sp.races = [];
    sp.factions = [];
    host.knowledge.social_power = sp;
    syncSocialPowerExtracted(tree, host.id);
    return true;
  }
  host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
  for (const r of pendingRaces) {
    const label = r.name.trim() || "种族";
    pushChildCard(tree, host, RACE_SLOT, label, formatWorldRaceExtracted(r), {
      world_race: normalizeRace(r),
    });
  }
  for (const f of pendingFactions) {
    const label = f.name.trim() || f.faction_type.trim() || "势力";
    pushChildCard(tree, host, FACTION_SLOT, label, formatMajorFactionExtracted(f), {
      major_faction: normalizeFaction(f),
    });
  }
  sp.races = [];
  sp.factions = [];
  host.knowledge.social_power = sp;
  syncSocialPowerExtracted(tree, host.id);
  return true;
}

export function createRaceCard(
  tree: NovelTree,
  hostId: string,
  label: string,
  race?: WorldRace,
): TreeNode | null {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host) return null;
  const id = crypto.randomUUID();
  const worldRace = normalizeRace(race ?? { ...emptyRace(), name: label });
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
      slot: RACE_SLOT,
      world_race: worldRace,
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
  syncSocialPowerExtracted(tree, hostId);
  return node;
}

export function createFactionCard(
  tree: NovelTree,
  hostId: string,
  label: string,
  faction?: MajorFaction,
): TreeNode | null {
  const host = tree.nodes.find((n) => n.id === hostId && n.kind === "knowledge");
  if (!host) return null;
  const id = crypto.randomUUID();
  const majorFaction = normalizeFaction(
    faction ?? { ...emptyFaction(), name: label, faction_type: label },
  );
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
      slot: FACTION_SLOT,
      major_faction: majorFaction,
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
  syncSocialPowerExtracted(tree, hostId);
  return node;
}

export function socialPowerHostId(tree: NovelTree, childId: string): string | null {
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === childId ? e.target : e.target === childId ? e.source : null;
    if (!other) continue;
    const n = tree.nodes.find((x) => x.id === other);
    if (n?.kind === "knowledge" && (n.knowledge?.slot ?? "") === "wv_social_power") return other;
  }
  for (const n of tree.nodes) {
    if (
      n.kind === "knowledge" &&
      (n.knowledge?.slot ?? "") === "wv_social_power" &&
      (n.linked_knowledge_ids ?? []).includes(childId)
    ) {
      return n.id;
    }
  }
  return null;
}

export function isSocialPowerChildSlot(slot: string | undefined | null): boolean {
  return isRaceSlot(slot) || isFactionSlot(slot);
}
