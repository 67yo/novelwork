import type { MessageKey } from "@/i18n/messages";
import type { NovelTree, TreeNode } from "@/lib/api";
import { linkedAxiomCards } from "@/lib/axiomCards";
import { linkedLocationCards } from "@/lib/locationCards";
import { linkedFactionCards, linkedRaceCards } from "@/lib/socialPowerCards";
import { linkedMajorEventCards, linkedReligionCards } from "@/lib/historyCultureCards";
import { linkedStoryRulesBlocks } from "@/lib/storyRulesCards";
import { isFactionSlot, isRaceSlot } from "@/lib/socialPower";
import { storyRulesFanTitleKey } from "@/lib/storyRules";
import {
  STORY_RULES_SLOT,
  WORLDVIEW_FAN_SLOTS,
  isStoryRulesSlot,
  knowledgeSlot,
  worldviewFanTitleKey,
} from "@/lib/worldview";
import {
  isWritePromptsSlot,
  writePromptSideFromEdge,
  WRITE_PROMPTS_SLOT,
  WRITE_PROMPTS_TITLE_KEY,
} from "@/lib/writePrompts";
import { rootLinkedKnowledgeIds, rootLinkedPlotIds, volumeLocalPlotIds, chapterLocalPlotIds } from "@/lib/treeLayout";
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
  if (slot === "wv_history_culture") {
    const merged = [
      ...linkedReligionCards(tree, parent.id),
      ...linkedMajorEventCards(tree, parent.id),
    ];
    return sortByLinkedOrder(parent, merged);
  }
  if (slot === "story_rules") {
    return sortByLinkedOrder(parent, linkedStoryRulesBlocks(tree, parent.id));
  }
  if (isWritePromptsSlot(slot)) {
    const kids = (parent.linked_knowledge_ids ?? [])
      .map((id) => tree.nodes.find((n) => n.id === id && n.kind === "knowledge"))
      .filter((n): n is TreeNode => !!n && !isWritePromptsSlot(knowledgeSlot(n)));
    return sortByLinkedOrder(parent, kids);
  }
  return [];
}

export function knowledgeNodeLabel(n: TreeNode, t: (key: MessageKey) => string): string {
  const slot = knowledgeSlot(n);
  const fanKey = worldviewFanTitleKey(slot);
  if (fanKey) return t(fanKey);
  const srKey = storyRulesFanTitleKey(slot);
  if (srKey) return t(srKey);
  if (isStoryRulesSlot(slot)) return t(STORY_RULES_SLOT.titleKey);
  if (isWritePromptsSlot(slot)) return t(WRITE_PROMPTS_TITLE_KEY);
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

/** 章 / 卷左栏关联知识：世界观六卡 + 故事规则 + 生成/精修仅隐藏展示 */
export function isHostPanelHiddenKnowledgeSlot(slot: string | undefined | null): boolean {
  const s = (slot ?? "").trim();
  return priorityRank(s) != null || isWritePromptsSlot(s);
}

export function filterHostPanelLinkedKnowledge(nodes: TreeNode[]): TreeNode[] {
  return nodes.filter((n) => !isHostPanelHiddenKnowledgeSlot(knowledgeSlot(n)));
}

/** 按 id 列表顺序排列节点（跳过缺失）；用于根→卷→章有效知识展示 */
export function orderKnowledgeNodesByIds(nodes: TreeNode[], ids: string[]): TreeNode[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n && n.kind === "knowledge") out.push(n);
  }
  return out;
}

/**
 * 根上 linked_knowledge_ids：固定槽（六世界观+故事规则）永远置顶且不参与用户排序；
 * `sortableIds` 为面板可拖拽项的新顺序。
 */
export function mergeRootKnowledgeOrder(
  nodes: TreeNode[],
  currentLinked: string[],
  sortableIds: string[],
): string[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const linkedSet = new Set(currentLinked);
  const fixed: string[] = [];
  for (const slot of KNOWLEDGE_PRIORITY_SLOTS) {
    const id = currentLinked.find((x) => {
      const n = byId.get(x);
      return n ? knowledgeSlot(n) === slot : false;
    });
    if (id) fixed.push(id);
  }
  const fixedSet = new Set(fixed);
  const sortable = sortableIds.filter((id) => linkedSet.has(id) && !fixedSet.has(id));
  const sortableSet = new Set(sortable);
  const rest = currentLinked.filter((id) => !fixedSet.has(id) && !sortableSet.has(id));
  return [...fixed, ...sortable, ...rest];
}

/** 无 host 顺序时的兜底（优先固定槽，其余按 id） */
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
    return a.id.localeCompare(b.id);
  });
}

function pushKnowledgeNavItem(
  n: TreeNode,
  indent: number,
  t: (key: MessageKey) => string,
  listed: Set<string>,
  out: KnowledgeNavItem[],
) {
  if (listed.has(n.id)) return;
  listed.add(n.id);
  out.push({
    id: n.id,
    label: knowledgeNodeLabel(n, t),
    kind: "knowledge",
    indent,
    chipShell: knowledgeChipShell(n),
  });
}

function knowledgeBySlot(tree: NovelTree): Map<string, TreeNode> {
  const bySlot = new Map<string, TreeNode>();
  for (const n of tree.nodes) {
    if (n.kind !== "knowledge") continue;
    const slot = knowledgeSlot(n);
    if (slot && !bySlot.has(slot)) bySlot.set(slot, n);
  }
  return bySlot;
}

export type WorldviewNavTone = "rose" | "indigo" | "amber" | "sky" | "teal";

export type WorldviewNavRow = {
  role: "parent" | "group" | "child";
  id: string;
  label: string;
  tone?: WorldviewNavTone;
};

function appendChildGroup(
  out: WorldviewNavRow[],
  t: (key: MessageKey) => string,
  parentId: string,
  labelKey: MessageKey,
  kids: TreeNode[],
  tone: WorldviewNavTone,
) {
  if (!kids.length) return;
  out.push({
    role: "group",
    id: `g:${parentId}:${labelKey}`,
    label: t(labelKey),
    tone,
  });
  for (const k of kids) {
    out.push({
      role: "child",
      id: k.id,
      label: knowledgeNodeLabel(k, t),
      tone,
    });
  }
}

/** 世界观六总卡 + 分类子卡（种族/势力等分组标题，无 ASCII 树） */
export function buildWorldviewNavItems(
  tree: NovelTree,
  t: (key: MessageKey) => string,
): WorldviewNavRow[] {
  const bySlot = knowledgeBySlot(tree);
  const out: WorldviewNavRow[] = [];
  for (const def of WORLDVIEW_FAN_SLOTS) {
    const parent = bySlot.get(def.slot);
    if (!parent) continue;
    out.push({
      role: "parent",
      id: parent.id,
      label: knowledgeNodeLabel(parent, t),
    });
    if (def.slot === "wv_core_laws") {
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.coreLaws.axioms",
        sortByLinkedOrder(parent, linkedAxiomCards(tree, parent.id)),
        "teal",
      );
    } else if (def.slot === "wv_spatiotemporal") {
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.st.locations",
        sortByLinkedOrder(parent, linkedLocationCards(tree, parent.id)),
        "teal",
      );
    } else if (def.slot === "wv_social_power") {
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.pow.races",
        sortByLinkedOrder(parent, linkedRaceCards(tree, parent.id)),
        "rose",
      );
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.pow.factions",
        sortByLinkedOrder(parent, linkedFactionCards(tree, parent.id)),
        "indigo",
      );
    } else if (def.slot === "wv_history_culture") {
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.hc.religions",
        sortByLinkedOrder(parent, linkedReligionCards(tree, parent.id)),
        "amber",
      );
      appendChildGroup(
        out,
        t,
        parent.id,
        "workspace.hc.majorEvents",
        sortByLinkedOrder(parent, linkedMajorEventCards(tree, parent.id)),
        "sky",
      );
    }
  }
  return out;
}

/** 全书：封面与规划 + 生成/精修（按左右边分子卡）+ 根上普通知识 */
export function buildBookNavItems(
  tree: NovelTree,
  t: (key: MessageKey) => string,
): WorldviewNavRow[] {
  const out: WorldviewNavRow[] = [];
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (root) {
    out.push({
      role: "parent",
      id: root.id,
      label: t("workspace.bookSelectRoot"),
    });
  }
  const hub = tree.nodes.find(
    (n) => n.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(n)),
  );
  const listed = new Set<string>();
  if (hub) {
    out.push({
      role: "parent",
      id: hub.id,
      label: knowledgeNodeLabel(hub, t),
    });
    listed.add(hub.id);
    const generate: TreeNode[] = [];
    const refine: TreeNode[] = [];
    const other: TreeNode[] = [];
    for (const k of linkedChildCards(tree, hub)) {
      listed.add(k.id);
      const edge = tree.edges.find(
        (e) =>
          (e.source === hub.id && e.target === k.id) ||
          (e.source === k.id && e.target === hub.id),
      );
      const side = edge ? writePromptSideFromEdge(hub.id, edge) : null;
      if (side === "generate") generate.push(k);
      else if (side === "refine") refine.push(k);
      else other.push(k);
    }
    appendChildGroup(
      out,
      t,
      hub.id,
      "workspace.writePrompts.handleGenerate",
      generate,
      "teal",
    );
    appendChildGroup(
      out,
      t,
      hub.id,
      "workspace.writePrompts.handleRefine",
      refine,
      "indigo",
    );
    for (const k of other) {
      out.push({
        role: "child",
        id: k.id,
        label: knowledgeNodeLabel(k, t),
        tone: "teal",
      });
    }
  }
  const generic = filterHostPanelLinkedKnowledge(
    orderKnowledgeNodesByIds(
      tree.nodes,
      rootLinkedKnowledgeIds(tree.nodes, tree.edges),
    ),
  ).filter((n) => !listed.has(n.id));
  if (generic.length) {
    out.push({
      role: "group",
      id: "g:book:knowledge",
      label: t("workspace.rootLinkedKnowledge"),
      tone: "teal",
    });
    for (const k of generic) {
      out.push({
        role: "child",
        id: k.id,
        label: knowledgeNodeLabel(k, t),
        tone: "teal",
      });
    }
  }
  return out;
}

function characterHostIds(tree: NovelTree, charId: string): string[] {
  const ids: string[] = [];
  for (const n of tree.nodes) {
    if (n.kind === "character") continue;
    if ((n.linked_character_ids ?? []).includes(charId)) ids.push(n.id);
  }
  for (const e of tree.edges) {
    if (e.kind !== "character") continue;
    const other = e.source === charId ? e.target : e.target === charId ? e.source : null;
    if (other && other !== charId) ids.push(other);
  }
  return [...new Set(ids)];
}

function primaryCharacterHost(tree: NovelTree, charId: string): TreeNode | null {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const hosts = characterHostIds(tree, charId)
    .map((id) => byId.get(id))
    .filter((n): n is TreeNode => !!n && n.kind !== "character");
  const rank = (k: string) =>
    k === "novel" ? 0 : k === "volume" ? 1 : k === "chapter" ? 2 : 3;
  hosts.sort((a, b) => rank(a.kind) - rank(b.kind) || a.id.localeCompare(b.id));
  return hosts[0] ?? null;
}

function characterNavHostLabel(host: TreeNode, t: (key: MessageKey) => string): string {
  if (host.kind === "novel") return t("workspace.tabBook");
  if (host.kind === "volume") return host.label?.trim() || t("workspace.volumeBadge");
  if (host.kind === "chapter") return host.label?.trim() || t("workspace.newChapter");
  if (host.kind === "side_plot") return host.label?.trim() || t("workspace.plot");
  return host.label?.trim() || host.id;
}

function sortCharactersForNav(host: TreeNode | null, kids: TreeNode[]): TreeNode[] {
  const order = host?.linked_character_ids ?? [];
  const rank = new Map(order.map((id, i) => [id, i]));
  return kids.slice().sort((a, b) => {
    const ia = rank.get(a.id) ?? 9999;
    const ib = rank.get(b.id) ?? 9999;
    return ia - ib || (a.label ?? "").localeCompare(b.label ?? "") || a.id.localeCompare(b.id);
  });
}

/** 人物：按宿主分组（全书 / 卷 / 章），无 ASCII 树 */
export function buildCharacterNavItems(
  tree: NovelTree,
  t: (key: MessageKey) => string,
): WorldviewNavRow[] {
  const buckets = new Map<string, { host: TreeNode; kids: TreeNode[] }>();
  const orphans: TreeNode[] = [];
  for (const n of tree.nodes) {
    if (n.kind !== "character") continue;
    const host = primaryCharacterHost(tree, n.id);
    if (!host) {
      orphans.push(n);
      continue;
    }
    const bucket = buckets.get(host.id) ?? { host, kids: [] };
    bucket.kids.push(n);
    buckets.set(host.id, bucket);
  }
  const hostRank = (k: string) =>
    k === "novel" ? 0 : k === "volume" ? 1 : k === "chapter" ? 2 : 3;
  const groups = [...buckets.values()].sort(
    (a, b) =>
      hostRank(a.host.kind) - hostRank(b.host.kind) ||
      a.host.position.y - b.host.position.y ||
      a.host.id.localeCompare(b.host.id),
  );
  const out: WorldviewNavRow[] = [];
  for (const { host, kids } of groups) {
    out.push({
      role: "group",
      id: `g:char:${host.id}`,
      label: characterNavHostLabel(host, t),
      tone: "amber",
    });
    for (const k of sortCharactersForNav(host, kids)) {
      out.push({
        role: "child",
        id: k.id,
        label: k.label?.trim() || t("workspace.newCharacter"),
        tone: "amber",
      });
    }
  }
  for (const k of sortCharactersForNav(null, orphans)) {
    out.push({
      role: "parent",
      id: k.id,
      label: k.label?.trim() || t("workspace.newCharacter"),
    });
  }
  return out;
}

function plotNavLabel(n: TreeNode, t: (key: MessageKey) => string): string {
  return n.label?.trim() || t("workspace.plot");
}

function appendPlotGroup(
  out: WorldviewNavRow[],
  listed: Set<string>,
  hostId: string,
  label: string,
  ids: string[],
  byId: Map<string, TreeNode>,
  t: (key: MessageKey) => string,
) {
  const kids = ids
    .map((id) => byId.get(id))
    .filter((n): n is TreeNode => !!n && n.kind === "side_plot");
  if (!kids.length) return;
  out.push({ role: "group", id: `g:plot:${hostId}`, label, tone: "sky" });
  for (const k of kids) {
    listed.add(k.id);
    out.push({
      role: "child",
      id: k.id,
      label: plotNavLabel(k, t),
      tone: "sky",
    });
  }
}

/** 自定义剧情：按全书 / 卷 / 章分组 */
export function buildPlotNavItems(
  tree: NovelTree,
  t: (key: MessageKey) => string,
): WorldviewNavRow[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const listed = new Set<string>();
  const out: WorldviewNavRow[] = [];
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (root) {
    appendPlotGroup(
      out,
      listed,
      root.id,
      t("workspace.tabBook"),
      rootLinkedPlotIds(tree.nodes, tree.edges),
      byId,
      t,
    );
  }
  const byY = (a: TreeNode, b: TreeNode) =>
    a.position.y - b.position.y || a.id.localeCompare(b.id);
  for (const vol of tree.nodes.filter((n) => n.kind === "volume").sort(byY)) {
    appendPlotGroup(
      out,
      listed,
      vol.id,
      vol.label?.trim() || t("workspace.volumeBadge"),
      volumeLocalPlotIds(vol.id, tree.nodes, tree.edges),
      byId,
      t,
    );
  }
  for (const ch of tree.nodes.filter((n) => n.kind === "chapter").sort(byY)) {
    appendPlotGroup(
      out,
      listed,
      ch.id,
      ch.label?.trim() || t("workspace.newChapter"),
      chapterLocalPlotIds(ch.id, tree.nodes, tree.edges),
      byId,
      t,
    );
  }
  for (const n of tree.nodes.filter((x) => x.kind === "side_plot" && !listed.has(x.id))) {
    out.push({ role: "parent", id: n.id, label: plotNavLabel(n, t) });
  }
  return out;
}

/** 故事规则总卡 + 四块（表层/引擎/兑现/红线） */
export function buildStoryRulesNavItems(
  tree: NovelTree,
  t: (key: MessageKey) => string,
): WorldviewNavRow[] {
  const hub = knowledgeBySlot(tree).get(STORY_RULES_SLOT.slot);
  if (!hub) return [];
  const out: WorldviewNavRow[] = [
    {
      role: "parent",
      id: hub.id,
      label: knowledgeNodeLabel(hub, t),
    },
  ];
  for (const k of linkedStoryRulesBlocks(tree, hub.id)) {
    out.push({
      role: "child",
      id: k.id,
      label: knowledgeNodeLabel(k, t),
      tone: "teal",
    });
  }
  return out;
}

/** 画布左侧控制台 · 知识卡列表 */
export function buildKnowledgeNavItems(tree: NovelTree, t: (key: MessageKey) => string): KnowledgeNavItem[] {
  const knowledgeNodes = tree.nodes.filter((n) => n.kind === "knowledge");
  const bySlot = knowledgeBySlot(tree);
  const listed = new Set<string>();
  const out: KnowledgeNavItem[] = [];

  for (const def of WORLDVIEW_FAN_SLOTS) {
    const parent = bySlot.get(def.slot);
    if (!parent) continue;
    pushKnowledgeNavItem(parent, 0, t, listed, out);
    for (const child of linkedChildCards(tree, parent)) {
      pushKnowledgeNavItem(child, 1, t, listed, out);
    }
  }

  const rules = bySlot.get(STORY_RULES_SLOT.slot);
  if (rules) {
    pushKnowledgeNavItem(rules, 0, t, listed, out);
    for (const child of linkedChildCards(tree, rules)) {
      pushKnowledgeNavItem(child, 1, t, listed, out);
    }
  }

  const writePrompts = bySlot.get(WRITE_PROMPTS_SLOT);
  if (writePrompts) {
    pushKnowledgeNavItem(writePrompts, 0, t, listed, out);
    for (const child of linkedChildCards(tree, writePrompts)) {
      pushKnowledgeNavItem(child, 1, t, listed, out);
    }
  }

  const rest = knowledgeNodes.filter((n) => !listed.has(n.id));
  rest.sort(
    (a, b) => knowledgeShuffleKey(a.id) - knowledgeShuffleKey(b.id) || a.id.localeCompare(b.id),
  );
  for (const n of rest) pushKnowledgeNavItem(n, 0, t, listed, out);

  return out;
}
