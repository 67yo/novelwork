import type { NovelTree, TreeNode } from "@/lib/api";
import {
  blockDataFromKnowledge,
  emptyConstraintRedlines,
  emptyFulfillmentSystem,
  emptyStoryEngine,
  emptySurfaceSetting,
  formatAllStoryRulesExtracted,
  formatStoryRulesBlockExtracted,
  isStoryRulesFanSlot,
  STORY_RULES_FAN_SLOTS,
  type StoryRulesBlockData,
  type StoryRulesBlockSlot,
} from "@/lib/storyRules";
import { isStoryRulesSlot, knowledgeSlot, STORY_RULES_SLOT } from "@/lib/worldview";

import type { MessageKey } from "@/i18n/messages";

type TitleFn = (key: MessageKey) => string;

function emptyPayloadForSlot(slot: StoryRulesBlockSlot) {
  const base = {
    book_ids: [],
    extract_prompt: "",
    extracted: "",
    slot,
    surface_setting: null,
    story_engine: null,
    fulfillment_system: null,
    constraint_redlines: null,
  };
  if (slot === "sr_surface_setting") {
    return { ...base, surface_setting: emptySurfaceSetting() };
  }
  if (slot === "sr_story_engine") {
    return { ...base, story_engine: emptyStoryEngine() };
  }
  if (slot === "sr_fulfillment_system") {
    return { ...base, fulfillment_system: emptyFulfillmentSystem() };
  }
  return { ...base, constraint_redlines: emptyConstraintRedlines() };
}

export function linkedStoryRulesBlocks(tree: NovelTree, rulesId: string): TreeNode[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const ids = new Set<string>();
  const rules = byId.get(rulesId);
  for (const id of rules?.linked_knowledge_ids ?? []) ids.add(id);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other =
      e.source === rulesId ? e.target : e.target === rulesId ? e.source : null;
    if (other) ids.add(other);
  }
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n?.kind === "knowledge" && isStoryRulesFanSlot(knowledgeSlot(n))) out.push(n);
  }
  out.sort((a, b) => {
    const sa = STORY_RULES_FAN_SLOTS.findIndex((s) => s.slot === knowledgeSlot(a));
    const sb = STORY_RULES_FAN_SLOTS.findIndex((s) => s.slot === knowledgeSlot(b));
    return sa - sb || a.position.x - b.position.x || a.id.localeCompare(b.id);
  });
  return out;
}

export function storyRulesBlockBySlot(
  tree: NovelTree,
  rulesId: string,
  slot: StoryRulesBlockSlot,
): TreeNode | undefined {
  return linkedStoryRulesBlocks(tree, rulesId).find((n) => knowledgeSlot(n) === slot);
}

export function syncStoryRulesParentExtracted(tree: NovelTree, rulesId: string, t: TitleFn): void {
  const rules = tree.nodes.find((n) => n.id === rulesId && n.kind === "knowledge");
  if (!rules?.knowledge) return;
  const blocks = STORY_RULES_FAN_SLOTS.map((def) => {
    const child = storyRulesBlockBySlot(tree, rulesId, def.slot);
    if (!child?.knowledge) return null;
    const data = blockDataFromKnowledge(def.slot, child.knowledge);
    return {
      slot: def.slot,
      data,
      title: t(def.titleKey),
    };
  }).filter(Boolean) as Array<{
    slot: StoryRulesBlockSlot;
    data: StoryRulesBlockData;
    title: string;
  }>;
  const extracted = formatAllStoryRulesExtracted(blocks);
  rules.knowledge.extracted = extracted;
  rules.outline = extracted ? extracted.slice(0, 200) : "";
}

/** 补齐故事规则右侧四卡；返回是否改树 */
export function ensureStoryRulesFanCards(tree: NovelTree, t: TitleFn): boolean {
  const rules = tree.nodes.find(
    (n) => n.kind === "knowledge" && isStoryRulesSlot(knowledgeSlot(n)),
  );
  if (!rules) return false;
  let changed = false;
  rules.linked_knowledge_ids = rules.linked_knowledge_ids ?? [];
  for (const def of STORY_RULES_FAN_SLOTS) {
    let node = tree.nodes.find(
      (n) => n.kind === "knowledge" && knowledgeSlot(n) === def.slot,
    );
    const title = t(def.titleKey);
    if (!node) {
      const id = crypto.randomUUID();
      const payload = emptyPayloadForSlot(def.slot);
      const data = blockDataFromKnowledge(def.slot, payload);
      const extracted = formatStoryRulesBlockExtracted(def.slot, data);
      node = {
        id,
        kind: "knowledge",
        label: title,
        outline: extracted.slice(0, 200),
        character: null,
        knowledge: { ...payload, extracted },
        side_plot: null,
        linked_character_ids: [],
        linked_side_plot_ids: [],
        linked_knowledge_ids: [],
        position: { ...rules.position },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
      };
      tree.nodes.push(node);
      changed = true;
    } else {
      if (node.label.trim() !== title) {
        node.label = title;
        changed = true;
      }
      if (node.knowledge && node.knowledge.slot !== def.slot) {
        node.knowledge.slot = def.slot;
        changed = true;
      }
    }
    if (!rules.linked_knowledge_ids.includes(node.id)) {
      rules.linked_knowledge_ids.push(node.id);
      changed = true;
    }
    let edge = tree.edges.find(
      (e) =>
        e.kind === "knowledge" &&
        ((e.source === rules.id && e.target === node!.id) ||
          (e.target === rules.id && e.source === node!.id)),
    );
    if (!edge) {
      tree.edges.push({
        id: `e-${rules.id}-${node.id}`,
        source: rules.id,
        target: node.id,
        kind: "knowledge",
        source_handle: "right",
        target_handle: "left",
      });
      changed = true;
    } else if (
      edge.source !== rules.id ||
      edge.target !== node.id ||
      edge.source_handle !== "right" ||
      edge.target_handle !== "left"
    ) {
      edge.source = rules.id;
      edge.target = node.id;
      edge.source_handle = "right";
      edge.target_handle = "left";
      changed = true;
    }
  }
  if (changed) syncStoryRulesParentExtracted(tree, rules.id, t);
  return changed;
}

export function findStoryRulesNode(tree: NovelTree): TreeNode | undefined {
  return tree.nodes.find(
    (n) => n.kind === "knowledge" && knowledgeSlot(n) === STORY_RULES_SLOT.slot,
  );
}
