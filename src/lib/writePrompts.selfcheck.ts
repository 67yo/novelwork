/** ponytail: write_prompts hub ensure / lock / layout split. Run: npx tsx src/lib/writePrompts.selfcheck.ts */
import { isHostPanelHiddenKnowledgeSlot } from "./knowledgeListSort.ts";
import { isFixedRootKnowledgeEdge, isFixedRootKnowledgeSlot } from "./worldview.ts";
import {
  WRITE_PROMPTS_SLOT,
  ensureWritePromptsCard,
  isWritePromptsRootEdge,
  isWritePromptsSlot,
  writePromptCardsForSide,
  writePromptEdgeHandles,
  writePromptSideFromEdge,
  writePromptsHub,
  writePromptsLinkHost,
} from "./writePrompts.ts";
import type { NovelTree } from "./api.ts";

const t = (k: string) => (k === "workspace.writePrompts.title" ? "生成/精修" : k);

function emptyTree(): NovelTree {
  return {
    novel_id: "n",
    nodes: [
      {
        id: "root",
        kind: "novel",
        label: "书",
        outline: "",
        detailed_outline: [],
        character: null,
        knowledge: null,
        side_plot: null,
        linked_character_ids: [],
        linked_side_plot_ids: [],
        linked_knowledge_ids: [],
        position: { x: 320, y: 0 },
        word_count: 0,
        word_count_min: 0,
        word_count_max: 0,
        chapter_count: 0,
      },
    ],
    edges: [],
  };
}

const tree = emptyTree();
console.assert(ensureWritePromptsCard(tree, t) === true, "first ensure creates");
console.assert(ensureWritePromptsCard(tree, t) === false, "ensure idempotent");
const hub = writePromptsHub(tree);
console.assert(hub && isWritePromptsSlot(hub.knowledge?.slot), "hub slot");
console.assert(isFixedRootKnowledgeSlot(WRITE_PROMPTS_SLOT), "slot is fixed for delete");
console.assert(isHostPanelHiddenKnowledgeSlot(WRITE_PROMPTS_SLOT), "hide hub in host panel");

const rootEdge = tree.edges.find((e) => e.source === "root" || e.target === "root")!;
console.assert(isWritePromptsRootEdge(tree.nodes, rootEdge), "root↔hub locked");
console.assert(isFixedRootKnowledgeEdge(tree.nodes, rootEdge), "fixed edge covers root↔hub");

tree.nodes.push({
  id: "g1",
  kind: "knowledge",
  label: "g1",
  outline: "",
  detailed_outline: [],
  character: null,
  knowledge: { book_ids: [], extract_prompt: "", extracted: "生成侧", slot: "" },
  side_plot: null,
  linked_character_ids: [],
  linked_side_plot_ids: [],
  linked_knowledge_ids: [],
  position: { x: -100, y: 0 },
  word_count: 0,
  word_count_min: 0,
  word_count_max: 0,
  chapter_count: 0,
});
tree.nodes.push({
  id: "r1",
  kind: "knowledge",
  label: "r1",
  outline: "",
  detailed_outline: [],
  character: null,
  knowledge: { book_ids: [], extract_prompt: "", extracted: "精修侧", slot: "" },
  side_plot: null,
  linked_character_ids: [],
  linked_side_plot_ids: [],
  linked_knowledge_ids: [],
  position: { x: 100, y: 0 },
  word_count: 0,
  word_count_min: 0,
  word_count_max: 0,
  chapter_count: 0,
});
const eGen = {
  id: "e-hub-g",
  source: hub!.id,
  target: "g1",
  kind: "knowledge",
  source_handle: "left",
  target_handle: "right",
};
const eRef = {
  id: "e-hub-r",
  source: hub!.id,
  target: "r1",
  kind: "knowledge",
  source_handle: "right",
  target_handle: "left",
};
tree.edges.push(eGen, eRef);
hub!.linked_knowledge_ids = ["g1", "r1"];

console.assert(!isWritePromptsRootEdge(tree.nodes, eGen), "child edge not root-locked");
console.assert(!isFixedRootKnowledgeEdge(tree.nodes, eGen), "child edge can unlink");
console.assert(writePromptSideFromEdge(hub!.id, eGen) === "generate", "left = generate");
console.assert(writePromptSideFromEdge(hub!.id, eRef) === "refine", "right = refine");
console.assert(
  writePromptCardsForSide(tree, "generate").map((n) => n.id).join() === "g1",
  "generate kids",
);
console.assert(
  writePromptCardsForSide(tree, "refine").map((n) => n.id).join() === "r1",
  "refine kids",
);
console.assert(writePromptEdgeHandles("generate").source === "left");
console.assert(writePromptEdgeHandles("refine").source === "right");

const pair = writePromptsLinkHost(
  tree.nodes.find((n) => n.id === hub!.id)!,
  tree.nodes.find((n) => n.id === "g1")!,
)!;
console.assert(pair.host.id === hub!.id && pair.card.id === "g1", "hub is always host");

const rootEdge2 = tree.edges.find((e) => e.source === "root" || e.target === "root")!;
console.assert(rootEdge2.source_handle === "wp", "root hub uses wp socket");

console.log("writePrompts.selfcheck ok");
