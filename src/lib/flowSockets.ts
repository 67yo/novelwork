import type { NovelTree, TreeEdge, TreeNode } from "@/lib/api";
import { isStoryRulesSlot, isWorldviewFanSlot, knowledgeSlot } from "@/lib/worldview";

export const SOCKET_TOP = "top";
export const SOCKET_BOTTOM = "bottom";
export const SOCKET_LEFT = "left";
export const SOCKET_RIGHT = "right";
/** 根 ↔ 世界大纲扇形 */
export const SOCKET_WV = "wv";
/** 根 ↔ 故事规则 */
export const SOCKET_SR = "sr";
/** 根 ↔ 生成/精修 */
export const SOCKET_WP = "wp";

export const CARD_SOCKETS = [SOCKET_TOP, SOCKET_BOTTOM, SOCKET_LEFT, SOCKET_RIGHT] as const;
export const ROOT_SOCKETS = [...CARD_SOCKETS, SOCKET_WV, SOCKET_SR, SOCKET_WP] as const;

export function socketsForKind(kind: string): readonly string[] {
  return kind === "novel" ? ROOT_SOCKETS : CARD_SOCKETS;
}

/** 根上固定知识卡用独立插座；普通知识仍走左←右。 */
export function rootKnowledgeHandles(slot: string): { source: string; target: string } | null {
  if ((slot ?? "").trim() === "write_prompts") return { source: SOCKET_WP, target: SOCKET_RIGHT };
  if (isStoryRulesSlot(slot)) return { source: SOCKET_SR, target: SOCKET_LEFT };
  if (isWorldviewFanSlot(slot)) return { source: SOCKET_WV, target: SOCKET_BOTTOM };
  return null;
}

export function migrateRootSpecialEdges(tree: NovelTree): boolean {
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (!root) return false;
  let changed = false;
  for (const e of tree.edges) {
    const otherId =
      e.source === root.id ? e.target : e.target === root.id ? e.source : null;
    if (!otherId) continue;
    const other = tree.nodes.find((n) => n.id === otherId);
    if (other?.kind !== "knowledge") continue;
    const h = rootKnowledgeHandles(knowledgeSlot(other));
    if (!h) continue;
    if (
      e.source !== root.id ||
      e.target !== other.id ||
      e.source_handle !== h.source ||
      e.target_handle !== h.target
    ) {
      e.source = root.id;
      e.target = other.id;
      e.source_handle = h.source;
      e.target_handle = h.target;
      changed = true;
    }
  }
  return changed;
}

export function isRootSpecialEdge(nodes: TreeNode[], edge: TreeEdge): boolean {
  const root = nodes.find((n) => n.kind === "novel");
  if (!root) return false;
  const otherId =
    edge.source === root.id ? edge.target : edge.target === root.id ? edge.source : null;
  if (!otherId) return false;
  const other = nodes.find((n) => n.id === otherId);
  if (other?.kind !== "knowledge") return false;
  return rootKnowledgeHandles(knowledgeSlot(other)) != null;
}
