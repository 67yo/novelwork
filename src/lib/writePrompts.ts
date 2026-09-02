import type { MessageKey } from "@/i18n/messages";
import type { NovelTree, TreeEdge, TreeNode } from "@/lib/api";
import { knowledgeSlot } from "@/lib/worldview";
import { migrateRootSpecialEdges } from "@/lib/flowSockets";

export const WRITE_PROMPTS_SLOT = "write_prompts" as const;
export const WRITE_PROMPTS_TITLE_KEY =
  "workspace.writePrompts.title" as const satisfies MessageKey;

export type WritePromptKind = "generate" | "refine";

export function isWritePromptsSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === WRITE_PROMPTS_SLOT;
}

export function writePromptsHub(tree: NovelTree): TreeNode | undefined {
  return tree.nodes.find(
    (n) => n.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(n)),
  );
}

export function isWritePromptsRootEdge(
  nodes: { id: string; kind: string; knowledge?: { slot?: string } | null }[],
  edge: { source: string; target: string },
): boolean {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const src = byId.get(edge.source);
  const tgt = byId.get(edge.target);
  if (!src || !tgt) return false;
  const hub =
    (src.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(src)) && src) ||
    (tgt.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(tgt)) && tgt) ||
    null;
  if (!hub) return false;
  const other = hub.id === src.id ? tgt : src;
  return other.kind === "novel";
}

type TitleFn = (key: MessageKey) => string;

/** 根上补齐生成/精修卡。已存在不改内容。 */
export function ensureWritePromptsCard(tree: NovelTree, t: TitleFn): boolean {
  const root = tree.nodes.find((n) => n.kind === "novel");
  if (!root) return false;
  const title = t(WRITE_PROMPTS_TITLE_KEY);
  let hub = writePromptsHub(tree);
  let changed = false;
  root.linked_knowledge_ids = root.linked_knowledge_ids ?? [];
  if (!hub) {
    const id = crypto.randomUUID();
    hub = {
      id,
      kind: "knowledge",
      label: title,
      outline: "",
      detailed_outline: [],
      character: null,
      knowledge: {
        book_ids: [],
        extract_prompt: "",
        extracted: "",
        slot: WRITE_PROMPTS_SLOT,
      },
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { ...root.position },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    tree.nodes.push(hub);
    changed = true;
  } else if (hub.label !== title) {
    hub.label = title;
    changed = true;
  }
  if (!root.linked_knowledge_ids.includes(hub.id)) {
    root.linked_knowledge_ids.push(hub.id);
    changed = true;
  }
  const hasEdge = tree.edges.some(
    (e) =>
      (e.source === root.id && e.target === hub!.id) ||
      (e.source === hub!.id && e.target === root.id),
  );
  if (!hasEdge) {
    tree.edges.push({
      id: `e-${root.id}-${hub.id}`,
      source: root.id,
      target: hub.id,
      kind: "knowledge",
      source_handle: "wp",
      target_handle: "right",
    });
    changed = true;
  }
  if (migrateRootSpecialEdges(tree)) changed = true;
  return changed;
}

export function writePromptEdgeHandles(side: WritePromptKind): {
  source: string;
  target: string;
} {
  return side === "refine"
    ? { source: "right", target: "left" }
    : { source: "left", target: "right" };
}

/** 生成/精修总卡上，某一侧挂着的知识卡（无边手柄的算生成侧）。 */
export function writePromptCardsForSide(
  tree: NovelTree,
  side: WritePromptKind,
): TreeNode[] {
  const hub = writePromptsHub(tree);
  if (!hub) return [];
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const out: TreeNode[] = [];
  const seen = new Set<string>();
  const take = (id: string) => {
    if (seen.has(id)) return;
    const n = byId.get(id);
    if (!n || n.kind !== "knowledge" || isWritePromptsSlot(knowledgeSlot(n))) return;
    const edge = tree.edges.find(
      (e) =>
        (e.source === hub.id && e.target === id) || (e.source === id && e.target === hub.id),
    );
    const s = edge ? writePromptSideFromEdge(hub.id, edge) : null;
    if (s !== side && !(side === "generate" && s == null)) return;
    seen.add(id);
    out.push(n);
  };
  for (const id of hub.linked_knowledge_ids ?? []) take(id);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === hub.id ? e.target : e.target === hub.id ? e.source : "";
    if (other) take(other);
  }
  return out;
}

export function writePromptSideFromEdge(
  hubId: string,
  edge: Pick<TreeEdge, "source" | "target" | "source_handle" | "target_handle">,
): WritePromptKind | null {
  const handle =
    edge.source === hubId
      ? edge.source_handle
      : edge.target === hubId
        ? edge.target_handle
        : null;
  const h = (handle ?? "").trim();
  if (h === "right") return "refine";
  if (h === "left") return "generate";
  return null;
}

/** 两端都是知识卡且其一为生成/精修时，宿主永远是本卡。 */
export function writePromptsLinkHost<
  T extends { id: string; kind: string; knowledge?: { slot?: string } | null },
>(a: T, b: T): { host: T; card: T } | null {
  const aHub = a.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(a));
  const bHub = b.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(b));
  if (aHub) return { host: a, card: b };
  if (bHub) return { host: b, card: a };
  return null;
}
