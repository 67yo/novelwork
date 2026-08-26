import type { NovelTree, TreeNode } from "@/lib/api";
import {
  AXIOM_SLOT,
  axiomHasContent,
  emptyAxiom,
  formatCoreLawsExtracted,
  formatWorldAxiomExtracted,
  isAxiomSlot,
  normalizeAxiom,
  normalizeCoreLaws,
  type WorldAxiom,
} from "@/lib/coreLaws";

/** 核心法则节点上挂着的世界公理子卡（边或 linked_knowledge_ids） */
export function linkedAxiomCards(tree: NovelTree, coreLawsId: string): TreeNode[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const ids = new Set<string>();
  const core = byId.get(coreLawsId);
  for (const id of core?.linked_knowledge_ids ?? []) ids.add(id);
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other =
      e.source === coreLawsId ? e.target : e.target === coreLawsId ? e.source : null;
    if (other) ids.add(other);
  }
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n?.kind === "knowledge" && isAxiomSlot(n.knowledge?.slot)) out.push(n);
  }
  out.sort(
    (a, b) =>
      a.position.x - b.position.x ||
      a.position.y - b.position.y ||
      a.id.localeCompare(b.id),
  );
  return out;
}

export function axiomsFromLinkedCards(tree: NovelTree, coreLawsId: string): WorldAxiom[] {
  return linkedAxiomCards(tree, coreLawsId).map((n) =>
    normalizeAxiom(n.knowledge?.world_axiom ?? emptyAxiom()),
  );
}

/** 用子卡内容重写核心法则 extracted（并清空内嵌 axioms） */
export function syncCoreLawsExtracted(tree: NovelTree, coreLawsId: string): void {
  const core = tree.nodes.find((n) => n.id === coreLawsId && n.kind === "knowledge");
  if (!core?.knowledge) return;
  const cl = normalizeCoreLaws(core.knowledge.core_laws);
  cl.axioms = [];
  core.knowledge.core_laws = cl;
  const extracted = formatCoreLawsExtracted(cl, axiomsFromLinkedCards(tree, coreLawsId));
  core.knowledge.extracted = extracted;
  core.outline = extracted ? [...extracted].slice(0, 200).join("") : "";
}

/** 把 core_laws.axioms 迁到扇形子卡；返回是否改树 */
export function migrateInlineAxiomsToCards(tree: NovelTree): boolean {
  const core = tree.nodes.find(
    (n) => n.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === "wv_core_laws",
  );
  if (!core?.knowledge?.core_laws) return false;
  const cl = normalizeCoreLaws(core.knowledge.core_laws);
  const pending = cl.axioms.filter(axiomHasContent);
  if (!pending.length) {
    if (cl.axioms.length) {
      cl.axioms = [];
      core.knowledge.core_laws = cl;
      syncCoreLawsExtracted(tree, core.id);
      return true;
    }
    return false;
  }
  if (linkedAxiomCards(tree, core.id).length) {
    cl.axioms = [];
    core.knowledge.core_laws = cl;
    syncCoreLawsExtracted(tree, core.id);
    return true;
  }
  core.linked_knowledge_ids = core.linked_knowledge_ids ?? [];
  for (const ax of pending) {
    const id = crypto.randomUUID();
    const label = ax.name.trim() || "世界公理";
    const extracted = formatWorldAxiomExtracted(ax);
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
        slot: AXIOM_SLOT,
        world_axiom: normalizeAxiom(ax),
      },
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { ...core.position },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    });
    core.linked_knowledge_ids.push(id);
    tree.edges.push({
      id: `e-${core.id}-${id}`,
      source: core.id,
      target: id,
      kind: "knowledge",
      source_handle: "top",
      target_handle: "bottom",
    });
  }
  cl.axioms = [];
  core.knowledge.core_laws = cl;
  syncCoreLawsExtracted(tree, core.id);
  return true;
}

export function createAxiomCard(
  tree: NovelTree,
  coreLawsId: string,
  label: string,
): TreeNode | null {
  const core = tree.nodes.find((n) => n.id === coreLawsId && n.kind === "knowledge");
  if (!core) return null;
  const id = crypto.randomUUID();
  const ax = emptyAxiom();
  ax.name = label;
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
      slot: AXIOM_SLOT,
      world_axiom: ax,
    },
    side_plot: null,
    linked_character_ids: [],
    linked_side_plot_ids: [],
    linked_knowledge_ids: [],
    position: { ...core.position },
    word_count: 0,
    word_count_min: 0,
    word_count_max: 0,
    chapter_count: 0,
  };
  tree.nodes.push(node);
  core.linked_knowledge_ids = core.linked_knowledge_ids ?? [];
  if (!core.linked_knowledge_ids.includes(id)) core.linked_knowledge_ids.push(id);
  tree.edges.push({
    id: `e-${core.id}-${id}`,
    source: core.id,
    target: id,
    kind: "knowledge",
    source_handle: "top",
    target_handle: "bottom",
  });
  syncCoreLawsExtracted(tree, coreLawsId);
  return node;
}

/** 公理卡宿主：核心法则节点 id */
export function axiomHostCoreLawsId(tree: NovelTree, axiomId: string): string | null {
  for (const e of tree.edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === axiomId ? e.target : e.target === axiomId ? e.source : null;
    if (!other) continue;
    const n = tree.nodes.find((x) => x.id === other);
    if (n?.kind === "knowledge" && (n.knowledge?.slot ?? "") === "wv_core_laws") return other;
  }
  for (const n of tree.nodes) {
    if (
      n.kind === "knowledge" &&
      (n.knowledge?.slot ?? "") === "wv_core_laws" &&
      (n.linked_knowledge_ids ?? []).includes(axiomId)
    ) {
      return n.id;
    }
  }
  return null;
}
