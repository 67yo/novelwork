import type { NovelTree, TreeNode } from "@/lib/api";
import { linkedAxiomCards } from "@/lib/axiomCards";
import { isAxiomSlot } from "@/lib/coreLaws";
import { linkedLocationCards } from "@/lib/locationCards";
import { isLocationSlot } from "@/lib/spatiotemporal";
import { linkedFactionCards, linkedRaceCards } from "@/lib/socialPowerCards";
import { isFactionSlot, isRaceSlot } from "@/lib/socialPower";
import {
  STORY_RULES_SLOT,
  WORLDVIEW_FAN_SLOTS,
  isStoryRulesSlot,
  knowledgeSlot,
  worldviewFanTitleKey,
} from "@/lib/worldview";
import { worldviewFanVisual } from "@/lib/worldviewStyle";

/** 控制台 / 关联知识：前 6 扇形 + 第 7 故事规则 */
export const KNOWLEDGE_PRIORITY_SLOTS = [
  ...WORLDVIEW_FAN_SLOTS.map((s) => s.slot),
  STORY_RULES_SLOT.slot,
] as const;

export type KnowledgeNavItem = {
  id: string;
  label: string;
  kind: "knowledge";
  indent: number;
  chipShell: string;
};

function isWorldviewChildSlot(slot: string): boolean {
  return isAxiomSlot(slot) || isLocationSlot(slot) || isRaceSlot(slot) || isFactionSlot(slot);
}

/** ponytail: 稳定伪随机，避免列表每次渲染乱序 */
function knowledgeShuffleKey(id: string): number {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function sortByLinkedOrder(host: TreeNode, children: TreeNode[]): TreeNode[] {
  const order = host.linked_knowledge_ids ?? [];
  const rank = new Map(order.map((id, i) => [id, i]));
  return children.slice().sort((a, b) => {
    const ia = rank.get(a.id) ?? 9999;
    const ib = rank.get(b.id) ?? 9999;
    return ia - ib || a.id.localeCompare(b.id);
  });
}

function linkedChildCards(tree: NovelTree, parent: TreeNode): TreeNode[] {
  const slot = knowledgeSlot(parent);
  if (slot === "wv_core_laws") return sortByLinkedOrder(parent, linkedAxiomCards(tree, parent.id));
  if (slot === "wv_spatiotemporal") {
    return sortByLinkedOrder(parent, linkedLocationCards(tree, parent.id));
  }
  if (slot === "wv_social_power") {
    const merged = [
      ...linkedRaceCards(tree, parent.id),
      ...linkedFactionCards(tree, parent.id),
    ];
    return sortByLinkedOrder(parent, merged);
  }
  return [];
}

export function knowledgeNodeLabel(n: TreeNode, t: (key: string) => string): string {
  const slot = knowledgeSlot(n);
  const fanKey = worldviewFanTitleKey(slot);
  if (fanKey) return t(fanKey);
  if (isStoryRulesSlot(slot)) return t(STORY_RULES_SLOT.titleKey);
  return n.label?.trim() || t("workspace.knowledgeCard");
}

/** 与画布 StoryNode 知识卡配色一致（去掉 min-width） */
export function knowledgeChipShell(n: TreeNode): string {
  const slot = knowledgeSlot(n);
  const fan = worldviewFanVisual(slot);
  if (fan) return fan.shell.replace(/\s*min-w-\[[^\]]+\]/g, "");
  if (isRaceSlot(slot)) return "border-rose-700/35 bg-[oklch(0.97_0.03_25)]";
  if (isFactionSlot(slot)) return "border-indigo-700/35 bg-[oklch(0.96_0.03_280)]";
  return "border-teal-700/30 bg-[oklch(0.96_0.03_175)]";
}

function priorityRank(slot: string): number | null {
  const idx = KNOWLEDGE_PRIORITY_SLOTS.indexOf(slot as (typeof KNOWLEDGE_PRIORITY_SLOTS)[number]);
  return idx >= 0 ? idx : null;
}

/** 章 / 卷左栏关联知识：世界观六卡 + 故事规则仅隐藏展示，关联仍生效 */
export function isHostPanelHiddenKnowledgeSlot(slot: string | undefined | null): boolean {
  return priorityRank((slot ?? "").trim()) != null;
}

export function filterHostPanelLinkedKnowledge(nodes: TreeNode[]): TreeNode[] {
  return nodes.filter((n) => !isHostPanelHiddenKnowledgeSlot(knowledgeSlot(n)));
}

/** 章节 / 卷「关联知识卡」可见项：其余稳定伪随机（前 7 槽在面板中隐藏） */
export function sortLinkedKnowledgeNodes(nodes: TreeNode[]): TreeNode[] {
  return nodes.slice().sort((a, b) => {
    const sa = knowledgeSlot(a);
    const sb = knowledgeSlot(b);
    const ra = priorityRank(sa);
    const rb = priorityRank(sb);
    if (ra != null || rb != null) {
      if (ra == null) return 1;
      if (rb == null) return -1;
      if (ra !== rb) return ra - rb;
      return a.id.localeCompare(b.id);
    }
    if (isWorldviewChildSlot(sa) && isWorldviewChildSlot(sb)) {
      return knowledgeShuffleKey(a.id) - knowledgeShuffleKey(b.id) || a.id.localeCompare(b.id);
    }
    return knowledgeShuffleKey(a.id) - knowledgeShuffleKey(b.id) || a.id.localeCompare(b.id);
  });
}

/** 画布左侧控制台 · 知识卡列表 */
export function buildKnowledgeNavItems(tree: NovelTree, t: (key: string) => string): KnowledgeNavItem[] {
  const knowledgeNodes = tree.nodes.filter((n) => n.kind === "knowledge");
  const bySlot = new Map<string, TreeNode>();
  for (const n of knowledgeNodes) {
    const slot = knowledgeSlot(n);
    if (slot && !bySlot.has(slot)) bySlot.set(slot, n);
  }
  const listed = new Set<string>();
  const out: KnowledgeNavItem[] = [];

  const push = (n: TreeNode, indent: number) => {
    if (listed.has(n.id)) return;
    listed.add(n.id);
    out.push({
      id: n.id,
      label: knowledgeNodeLabel(n, t),
      kind: "knowledge",
      indent,
      chipShell: knowledgeChipShell(n),
    });
  };

  for (const def of WORLDVIEW_FAN_SLOTS) {
    const parent = bySlot.get(def.slot);
    if (!parent) continue;
    push(parent, 0);
    for (const child of linkedChildCards(tree, parent)) push(child, 1);
  }

  const rules = bySlot.get(STORY_RULES_SLOT.slot);
  if (rules) push(rules, 0);

  const rest = knowledgeNodes.filter((n) => !listed.has(n.id));
  rest.sort(
    (a, b) => knowledgeShuffleKey(a.id) - knowledgeShuffleKey(b.id) || a.id.localeCompare(b.id),
  );
  for (const n of rest) push(n, 0);

  return out;
}
