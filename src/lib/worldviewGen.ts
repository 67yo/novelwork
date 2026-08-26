import type { MessageKey } from "@/i18n/messages";
import type { NovelTree, TreeNode } from "@/lib/api";
import { linkedAxiomCards, syncCoreLawsExtracted } from "@/lib/axiomCards";
import {
  axiomHasContent,
  formatWorldAxiomExtracted,
  normalizeAxiom,
  normalizeCoreLaws,
  type CoreLawsData,
  type WorldAxiom,
} from "@/lib/coreLaws";
import {
  formatExistenceExtracted,
  normalizeExistence,
  type ExistenceData,
} from "@/lib/existence";
import {
  formatMajorEventExtracted,
  formatWorldReligionExtracted,
  majorEventHasContent,
  normalizeHistoryCulture,
  normalizeMajorEvent,
  normalizeReligion,
  religionHasContent,
  type HistoryCultureData,
  type MajorEvent,
  type WorldReligion,
} from "@/lib/historyCulture";
import {
  linkedMajorEventCards,
  linkedReligionCards,
  migrateInlineHistoryCultureToCards,
  syncHistoryCultureExtracted,
} from "@/lib/historyCultureCards";
import {
  formatInfoFlowExtracted,
  normalizeInfoFlow,
  type InfoFlowData,
} from "@/lib/infoFlow";
import {
  linkedLocationCards,
  migrateInlineLocationsToCards,
  syncSpatiotemporalExtracted,
} from "@/lib/locationCards";
import {
  locationHasContent,
  normalizeLocation,
  normalizeSpatiotemporal,
  type KeyLocation,
  type SpatiotemporalData,
} from "@/lib/spatiotemporal";
import {
  linkedFactionCards,
  linkedRaceCards,
  migrateInlineSocialPowerToCards,
  syncSocialPowerExtracted,
} from "@/lib/socialPowerCards";
import {
  factionHasContent,
  normalizeFaction,
  normalizeRace,
  normalizeSocialPower,
  raceHasContent,
  type MajorFaction,
  type SocialPowerData,
  type WorldRace,
} from "@/lib/socialPower";
import {
  ensureWorldviewCards,
  knowledgeSlot,
  type WorldviewSlotId,
} from "@/lib/worldview";

export type WorldviewGenPayload = {
  core_laws?: Partial<CoreLawsData> & { axioms?: WorldAxiom[] };
  spatiotemporal?: Partial<SpatiotemporalData> & { locations?: KeyLocation[] };
  social_power?: Partial<SocialPowerData> & { races?: WorldRace[]; factions?: MajorFaction[] };
  existence?: Partial<ExistenceData>;
  info_flow?: Partial<InfoFlowData>;
  history_culture?: Partial<HistoryCultureData> & {
    religions?: WorldReligion[];
    major_events?: MajorEvent[];
    /** 兼容旧 LLM 仅输出 extracted */
    extracted?: string;
  };
  story_rules?: { extracted?: string };
};

type TitleFn = (key: MessageKey) => string;

function nodeBySlot(tree: NovelTree, slot: WorldviewSlotId): TreeNode | undefined {
  return tree.nodes.find((n) => n.kind === "knowledge" && knowledgeSlot(n) === slot);
}

function removeChildCards(tree: NovelTree, hostId: string, children: TreeNode[]): void {
  if (!children.length) return;
  const drop = new Set(children.map((c) => c.id));
  tree.nodes = tree.nodes.filter((n) => !drop.has(n.id));
  tree.edges = tree.edges.filter((e) => !drop.has(e.source) && !drop.has(e.target));
  const host = tree.nodes.find((n) => n.id === hostId);
  if (host?.linked_knowledge_ids?.length) {
    host.linked_knowledge_ids = host.linked_knowledge_ids.filter((id) => !drop.has(id));
  }
}

function pushChild(
  tree: NovelTree,
  host: TreeNode,
  slot: string,
  label: string,
  extracted: string,
  extra: Record<string, unknown>,
): void {
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
      core_laws: null,
      spatiotemporal: null,
      social_power: null,
      existence: null,
      info_flow: null,
      history_culture: null,
      world_axiom: null,
      key_location: null,
      world_race: null,
      major_faction: null,
      world_religion: null,
      major_event: null,
      ...extra,
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
}

function applyCoreLaws(tree: NovelTree, raw: WorldviewGenPayload["core_laws"]): boolean {
  const host = nodeBySlot(tree, "wv_core_laws");
  if (!host?.knowledge || !raw) return false;
  removeChildCards(tree, host.id, linkedAxiomCards(tree, host.id));
  const cl = normalizeCoreLaws({ ...host.knowledge.core_laws, ...raw });
  cl.axioms = (raw.axioms ?? []).map((a) => normalizeAxiom(a)).filter(axiomHasContent);
  host.knowledge.core_laws = cl;
  for (const ax of cl.axioms) {
    const label = ax.name.trim() || "世界公理";
    pushChild(tree, host, "wv_axiom", label, formatWorldAxiomExtracted(ax), {
      world_axiom: normalizeAxiom(ax),
    });
  }
  cl.axioms = [];
  host.knowledge.core_laws = cl;
  syncCoreLawsExtracted(tree, host.id);
  return true;
}

function applySpatiotemporal(tree: NovelTree, raw: WorldviewGenPayload["spatiotemporal"]): boolean {
  const host = nodeBySlot(tree, "wv_spatiotemporal");
  if (!host?.knowledge || !raw) return false;
  removeChildCards(tree, host.id, linkedLocationCards(tree, host.id));
  const st = normalizeSpatiotemporal({ ...host.knowledge.spatiotemporal, ...raw });
  const locs = (raw.locations ?? []).map((l) => normalizeLocation(l)).filter(locationHasContent);
  st.locations = locs;
  host.knowledge.spatiotemporal = st;
  migrateInlineLocationsToCards(tree);
  syncSpatiotemporalExtracted(tree, host.id);
  return true;
}

function applySocialPower(tree: NovelTree, raw: WorldviewGenPayload["social_power"]): boolean {
  const host = nodeBySlot(tree, "wv_social_power");
  if (!host?.knowledge || !raw) return false;
  removeChildCards(tree, host.id, [
    ...linkedRaceCards(tree, host.id),
    ...linkedFactionCards(tree, host.id),
  ]);
  const sp = normalizeSocialPower({ ...host.knowledge.social_power, ...raw });
  sp.races = (raw.races ?? []).map((r) => normalizeRace(r)).filter(raceHasContent);
  sp.factions = (raw.factions ?? []).map((f) => normalizeFaction(f)).filter(factionHasContent);
  host.knowledge.social_power = sp;
  migrateInlineSocialPowerToCards(tree);
  syncSocialPowerExtracted(tree, host.id);
  return true;
}

function applyExistence(tree: NovelTree, raw: WorldviewGenPayload["existence"]): boolean {
  const host = nodeBySlot(tree, "wv_existence");
  if (!host?.knowledge || !raw) return false;
  const ex = normalizeExistence({ ...host.knowledge.existence, ...raw });
  host.knowledge.existence = ex;
  const extracted = formatExistenceExtracted(ex);
  host.knowledge.extracted = extracted;
  host.outline = extracted.slice(0, 200);
  return true;
}

function applyInfoFlow(tree: NovelTree, raw: WorldviewGenPayload["info_flow"]): boolean {
  const host = nodeBySlot(tree, "wv_info_flow");
  if (!host?.knowledge || !raw) return false;
  const info = normalizeInfoFlow({ ...host.knowledge.info_flow, ...raw });
  host.knowledge.info_flow = info;
  const extracted = formatInfoFlowExtracted(info);
  host.knowledge.extracted = extracted;
  host.outline = extracted.slice(0, 200);
  return true;
}

function applyHistoryCulture(
  tree: NovelTree,
  raw: NonNullable<WorldviewGenPayload["history_culture"]>,
): boolean {
  const host = nodeBySlot(tree, "wv_history_culture");
  if (!host?.knowledge) return false;

  if (raw.extracted != null && String(raw.extracted).trim()) {
    const text = String(raw.extracted).trim();
    host.knowledge.extracted = text;
    host.outline = text.slice(0, 200);
    return true;
  }

  removeChildCards(tree, host.id, [
    ...linkedReligionCards(tree, host.id),
    ...linkedMajorEventCards(tree, host.id),
  ]);
  const hc = normalizeHistoryCulture({ ...host.knowledge.history_culture, ...raw });
  const religions = (raw.religions ?? []).map((r) => normalizeReligion(r)).filter(religionHasContent);
  const events = (raw.major_events ?? [])
    .map((e) => normalizeMajorEvent(e))
    .filter(majorEventHasContent);
  hc.religions = religions;
  hc.major_events = events;
  host.knowledge.history_culture = hc;
  for (const rel of religions) {
    const label = rel.name.trim() || "宗教";
    pushChild(tree, host, "wv_religion", label, formatWorldReligionExtracted(rel), {
      world_religion: normalizeReligion(rel),
    });
  }
  for (const ev of events) {
    const label = ev.title.trim() || "重大事件";
    pushChild(tree, host, "wv_major_event", label, formatMajorEventExtracted(ev), {
      major_event: normalizeMajorEvent(ev),
    });
  }
  hc.religions = [];
  hc.major_events = [];
  host.knowledge.history_culture = hc;
  migrateInlineHistoryCultureToCards(tree);
  syncHistoryCultureExtracted(tree, host.id);
  return true;
}

function applyExtractedSlot(tree: NovelTree, slot: WorldviewSlotId, extracted: string): boolean {
  const host = nodeBySlot(tree, slot);
  if (!host?.knowledge) return false;
  const text = extracted.trim();
  host.knowledge.extracted = text;
  host.outline = text.slice(0, 200);
  return true;
}

export function normalizeWorldviewGenPayload(raw: unknown): WorldviewGenPayload | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const w = (o.worldview ?? o) as Record<string, unknown>;
  if (!w || typeof w !== "object") return null;
  return w as WorldviewGenPayload;
}

/** 将 LLM 返回的 worldview JSON 写入树上各槽位（含子卡）。调用前应先 ensureWorldviewCards。 */
export function applyWorldviewPayload(
  tree: NovelTree,
  raw: unknown,
  t: TitleFn,
): boolean {
  ensureWorldviewCards(tree, t);
  const p = normalizeWorldviewGenPayload(raw);
  if (!p) return false;
  let changed = false;
  if (p.core_laws) changed = applyCoreLaws(tree, p.core_laws) || changed;
  if (p.spatiotemporal) changed = applySpatiotemporal(tree, p.spatiotemporal) || changed;
  if (p.social_power) changed = applySocialPower(tree, p.social_power) || changed;
  if (p.existence) changed = applyExistence(tree, p.existence) || changed;
  if (p.info_flow) changed = applyInfoFlow(tree, p.info_flow) || changed;
  if (p.history_culture) changed = applyHistoryCulture(tree, p.history_culture) || changed;
  if (p.story_rules?.extracted != null) {
    changed = applyExtractedSlot(tree, "story_rules", String(p.story_rules.extracted)) || changed;
  }
  return changed;
}
