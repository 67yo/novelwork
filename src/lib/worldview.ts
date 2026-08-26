import type { MessageKey } from "@/i18n/messages";
import type { NovelTree, TreeNode } from "@/lib/api";
import { normalizeCoreLaws } from "@/lib/coreLaws";
import { normalizeExistence } from "@/lib/existence";
import { normalizeInfoFlow } from "@/lib/infoFlow";
import { normalizeHistoryCulture } from "@/lib/historyCulture";
import { normalizeSpatiotemporal } from "@/lib/spatiotemporal";
import { normalizeSocialPower } from "@/lib/socialPower";

/** 根向上扇形：世界观六卡（不含右侧故事规则） */
export const WORLDVIEW_FAN_SLOTS = [
  { slot: "wv_core_laws", titleKey: "workspace.wv.coreLaws" },
  { slot: "wv_spatiotemporal", titleKey: "workspace.wv.spatiotemporal" },
  { slot: "wv_social_power", titleKey: "workspace.wv.socialPower" },
  { slot: "wv_history_culture", titleKey: "workspace.wv.historyCulture" },
  { slot: "wv_existence", titleKey: "workspace.wv.existence" },
  { slot: "wv_info_flow", titleKey: "workspace.wv.infoFlow" },
] as const;

export const STORY_RULES_SLOT = {
  slot: "story_rules",
  titleKey: "workspace.wv.storyRules",
} as const;

export type WorldviewSlotId =
  | (typeof WORLDVIEW_FAN_SLOTS)[number]["slot"]
  | typeof STORY_RULES_SLOT.slot;

import { isStoryRulesFanSlot } from "@/lib/storyRules";

const FAN_SLOT_SET = new Set<string>(WORLDVIEW_FAN_SLOTS.map((s) => s.slot));

export function isWorldviewFanSlot(slot: string | undefined | null): boolean {
  return !!slot && FAN_SLOT_SET.has(slot);
}

/** 扇形六卡 i18n 标题键；用于固定标题展示 */
export function worldviewFanTitleKey(
  slot: string | undefined | null,
): (typeof WORLDVIEW_FAN_SLOTS)[number]["titleKey"] | null {
  if (!slot) return null;
  const def = WORLDVIEW_FAN_SLOTS.find((s) => s.slot === slot);
  return def?.titleKey ?? null;
}

export function isStoryRulesSlot(slot: string | undefined | null): boolean {
  return slot === STORY_RULES_SLOT.slot;
}

export function isFixedRootKnowledgeSlot(slot: string | undefined | null): boolean {
  return isWorldviewFanSlot(slot) || isStoryRulesSlot(slot) || isStoryRulesFanSlot(slot);
}

/** 边是否连着世界观六卡 / 故事规则（不可切断） */
export function isFixedRootKnowledgeEdge(
  nodes: { id: string; kind: string; knowledge?: { slot?: string } | null }[],
  edge: { source: string; target: string },
): boolean {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  for (const id of [edge.source, edge.target]) {
    const n = byId.get(id);
    if (n?.kind === "knowledge" && isFixedRootKnowledgeSlot(knowledgeSlot(n))) return true;
  }
  return false;
}

export function knowledgeSlot(n: {
  knowledge?: { slot?: string } | null;
}): string {
  return (n.knowledge?.slot ?? "").trim();
}

export function allWorldviewDefs() {
  return [...WORLDVIEW_FAN_SLOTS, STORY_RULES_SLOT];
}

/** 缺哪些固定槽；已有同标题挂在根上的普通知识卡会认领为该槽。 */
export function missingWorldviewSlots(tree: NovelTree): WorldviewSlotId[] {
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (!root) return allWorldviewDefs().map((d) => d.slot);
  const bySlot = new Map<string, TreeNode>();
  for (const n of tree.nodes) {
    if (n.kind !== "knowledge") continue;
    const s = knowledgeSlot(n);
    if (s) bySlot.set(s, n);
  }
  const missing: WorldviewSlotId[] = [];
  for (const def of allWorldviewDefs()) {
    if (bySlot.has(def.slot)) continue;
    missing.push(def.slot);
  }
  return missing;
}

export function worldviewComplete(tree: NovelTree): boolean {
  return missingWorldviewSlots(tree).length === 0;
}

type TitleFn = (key: MessageKey) => string;

export function worldviewFanTitle(t: TitleFn, slot: string): string {
  const key = worldviewFanTitleKey(slot);
  return key ? t(key) : "";
}

/**
 * 在根上补齐世界观六卡 + 故事规则；已存在槽位不改内容。
 * @returns 是否新建了节点
 */
export function ensureWorldviewCards(tree: NovelTree, t: TitleFn): boolean {
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (!root) return false;

  const bySlot = new Map<string, TreeNode>();
  for (const n of tree.nodes) {
    if (n.kind !== "knowledge") continue;
    const s = knowledgeSlot(n);
    if (s) bySlot.set(s, n);
  }

  // 认领：根上已挂、无 slot、标题完全匹配的知识卡
  const rootKnowIds = new Set(root.linked_knowledge_ids ?? []);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === root.id ? e.target : e.target === root.id ? e.source : null;
    if (other) rootKnowIds.add(other);
  }

  let changed = false;
  root.linked_knowledge_ids = root.linked_knowledge_ids ?? [];

  for (const def of allWorldviewDefs()) {
    let node = bySlot.get(def.slot);
    const title = t(def.titleKey);
    if (!node) {
      const orphan = tree.nodes.find(
        (n) =>
          n.kind === "knowledge" &&
          !knowledgeSlot(n) &&
          n.label.trim() === title &&
          rootKnowIds.has(n.id),
      );
      if (orphan) {
        orphan.knowledge = orphan.knowledge ?? {
          book_ids: [],
          extract_prompt: "",
          extracted: "",
        };
        orphan.knowledge.slot = def.slot;
        if (def.slot === "wv_core_laws" && !orphan.knowledge.core_laws) {
          orphan.knowledge.core_laws = normalizeCoreLaws(null);
        }
        if (def.slot === "wv_spatiotemporal" && !orphan.knowledge.spatiotemporal) {
          orphan.knowledge.spatiotemporal = normalizeSpatiotemporal(null);
        }
        if (def.slot === "wv_social_power" && !orphan.knowledge.social_power) {
          orphan.knowledge.social_power = normalizeSocialPower(null);
        }
        if (def.slot === "wv_existence" && !orphan.knowledge.existence) {
          orphan.knowledge.existence = normalizeExistence(null);
        }
        if (def.slot === "wv_info_flow" && !orphan.knowledge.info_flow) {
          orphan.knowledge.info_flow = normalizeInfoFlow(null);
        }
        if (def.slot === "wv_history_culture" && !orphan.knowledge.history_culture) {
          orphan.knowledge.history_culture = normalizeHistoryCulture(null);
        }
        orphan.label = title;
        node = orphan;
        bySlot.set(def.slot, orphan);
        changed = true;
      }
    }
    if (!node) {
      const id = crypto.randomUUID();
      const isFan = isWorldviewFanSlot(def.slot);
      node = {
        id,
        kind: "knowledge",
        label: title,
        outline: "",
        character: null,
        knowledge: {
          book_ids: [],
          extract_prompt: "",
          extracted: "",
          slot: def.slot,
          core_laws: def.slot === "wv_core_laws" ? normalizeCoreLaws(null) : null,
          spatiotemporal:
            def.slot === "wv_spatiotemporal" ? normalizeSpatiotemporal(null) : null,
          social_power:
            def.slot === "wv_social_power" ? normalizeSocialPower(null) : null,
          existence: def.slot === "wv_existence" ? normalizeExistence(null) : null,
          info_flow: def.slot === "wv_info_flow" ? normalizeInfoFlow(null) : null,
          history_culture:
            def.slot === "wv_history_culture" ? normalizeHistoryCulture(null) : null,
          world_axiom: null,
          key_location: null,
          world_race: null,
          major_faction: null,
          world_religion: null,
          major_event: null,
        },
        side_plot: null,
        linked_character_ids: [],
        linked_side_plot_ids: [],
        linked_knowledge_ids: [],
        position: { x: root.position.x, y: root.position.y },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
      };
      tree.nodes.push(node);
      bySlot.set(def.slot, node);
      changed = true;
      if (!root.linked_knowledge_ids.includes(id)) {
        root.linked_knowledge_ids.push(id);
      }
      tree.edges.push({
        id: `e-${root.id}-${id}`,
        source: root.id,
        target: id,
        kind: "knowledge",
        source_handle: isFan ? "top" : "right",
        target_handle: isFan ? "bottom" : "left",
      });
    } else {
      if (!root.linked_knowledge_ids.includes(node.id)) {
        root.linked_knowledge_ids.push(node.id);
        changed = true;
      }
      const isFan = isWorldviewFanSlot(def.slot);
      let edge = tree.edges.find(
        (e) =>
          e.kind === "knowledge" &&
          ((e.source === root.id && e.target === node!.id) ||
            (e.target === root.id && e.source === node!.id)),
      );
      if (!edge) {
        tree.edges.push({
          id: `e-${root.id}-${node.id}`,
          source: root.id,
          target: node.id,
          kind: "knowledge",
          source_handle: isFan ? "top" : "right",
          target_handle: isFan ? "bottom" : "left",
        });
        changed = true;
      } else {
        const sh = isFan ? "top" : "right";
        const th = isFan ? "bottom" : "left";
        if (
          edge.source !== root.id ||
          edge.target !== node.id ||
          edge.source_handle !== sh ||
          edge.target_handle !== th
        ) {
          edge.source = root.id;
          edge.target = node.id;
          edge.source_handle = sh;
          edge.target_handle = th;
          changed = true;
        }
      }
      if (node.knowledge && node.knowledge.slot !== def.slot) {
        node.knowledge.slot = def.slot;
        changed = true;
      }
      if (node.label.trim() !== title) {
        node.label = title;
        changed = true;
      }
    }
  }
  return changed;
}
