import type { MessageKey } from "@/i18n/messages";
import type { NovelTree } from "@/lib/api";
import {
  blockDataFromKnowledge,
  formatStoryRulesBlockExtracted,
  normalizeConstraintRedlines,
  normalizeFulfillmentSystem,
  normalizeStoryEngine,
  normalizeSurfaceSetting,
  STORY_RULES_FAN_SLOTS,
  type ConstraintRedlinesData,
  type FulfillmentSystemData,
  type StoryEngineData,
  type StoryRulesBlockSlot,
  type SurfaceSettingData,
} from "@/lib/storyRules";
import {
  ensureStoryRulesFanCards,
  findStoryRulesNode,
  storyRulesBlockBySlot,
  syncStoryRulesParentExtracted,
} from "@/lib/storyRulesCards";
import { knowledgeSlot } from "@/lib/worldview";

export type StoryRulesGenPayload = {
  surface_setting?: Partial<SurfaceSettingData>;
  story_engine?: Partial<StoryEngineData>;
  fulfillment_system?: Partial<FulfillmentSystemData>;
  constraint_redlines?: Partial<ConstraintRedlinesData>;
};

type TitleFn = (key: MessageKey) => string;

function applyBlock(
  tree: NovelTree,
  rulesId: string,
  slot: StoryRulesBlockSlot,
  raw: Record<string, unknown> | undefined,
  t: TitleFn,
): boolean {
  if (!raw) return false;
  const node = storyRulesBlockBySlot(tree, rulesId, slot);
  if (!node?.knowledge) return false;
  let data: Record<string, unknown>;
  let payloadKey: string;
  if (slot === "sr_surface_setting") {
    data = normalizeSurfaceSetting(raw as SurfaceSettingData);
    payloadKey = "surface_setting";
  } else if (slot === "sr_story_engine") {
    data = normalizeStoryEngine(raw as StoryEngineData);
    payloadKey = "story_engine";
  } else if (slot === "sr_fulfillment_system") {
    data = normalizeFulfillmentSystem(raw as FulfillmentSystemData);
    payloadKey = "fulfillment_system";
  } else {
    data = normalizeConstraintRedlines(raw as ConstraintRedlinesData);
    payloadKey = "constraint_redlines";
  }
  const extracted = formatStoryRulesBlockExtracted(slot, data as never);
  (node.knowledge as Record<string, unknown>)[payloadKey] = data;
  node.knowledge.extracted = extracted;
  node.outline = extracted.slice(0, 200);
  const title = t(STORY_RULES_FAN_SLOTS.find((s) => s.slot === slot)!.titleKey);
  if (node.label.trim() !== title) node.label = title;
  return true;
}

/** 将 Chat 返回的四卡 JSON 写入树上 */
export function applyStoryRulesPayload(
  tree: NovelTree,
  payload: StoryRulesGenPayload,
  t: TitleFn,
): boolean {
  ensureStoryRulesFanCards(tree, t);
  const rules = findStoryRulesNode(tree);
  if (!rules) return false;
  let changed = false;
  if (applyBlock(tree, rules.id, "sr_surface_setting", payload.surface_setting, t)) changed = true;
  if (applyBlock(tree, rules.id, "sr_story_engine", payload.story_engine, t)) changed = true;
  if (applyBlock(tree, rules.id, "sr_fulfillment_system", payload.fulfillment_system, t)) changed = true;
  if (applyBlock(tree, rules.id, "sr_constraint_redlines", payload.constraint_redlines, t)) changed = true;
  if (changed) {
    syncStoryRulesParentExtracted(tree, rules.id, t);
  }
  return changed;
}

export function storyRulesBlocksSnapshot(tree: NovelTree): StoryRulesGenPayload {
  const rules = findStoryRulesNode(tree);
  if (!rules) return {};
  const out: StoryRulesGenPayload = {};
  for (const def of STORY_RULES_FAN_SLOTS) {
    const node = storyRulesBlockBySlot(tree, rules.id, def.slot);
    if (!node?.knowledge) continue;
    const data = blockDataFromKnowledge(def.slot, node.knowledge);
    if (def.slot === "sr_surface_setting") out.surface_setting = data as SurfaceSettingData;
    else if (def.slot === "sr_story_engine") out.story_engine = data as StoryEngineData;
    else if (def.slot === "sr_fulfillment_system") out.fulfillment_system = data as FulfillmentSystemData;
    else out.constraint_redlines = data as ConstraintRedlinesData;
  }
  return out;
}

export function isStoryRulesBlockSlot(slot: string): boolean {
  return STORY_RULES_FAN_SLOTS.some((s) => s.slot === slot);
}

export function storyRulesBlockSlotOf(n: { knowledge?: { slot?: string } | null }): StoryRulesBlockSlot | "" {
  const slot = knowledgeSlot(n);
  return isStoryRulesBlockSlot(slot) ? (slot as StoryRulesBlockSlot) : "";
}
