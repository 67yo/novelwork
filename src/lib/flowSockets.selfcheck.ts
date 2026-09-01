/** ponytail: root wv/sr/wp handles. Run: npx tsx src/lib/flowSockets.selfcheck.ts */
import {
  SOCKET_SR,
  SOCKET_WV,
  SOCKET_WP,
  migrateRootSpecialEdges,
  rootKnowledgeHandles,
  socketsForKind,
} from "./flowSockets.ts";
import type { NovelTree } from "./api.ts";

console.assert(socketsForKind("chapter").join() === "top,bottom,left,right");
console.assert(socketsForKind("novel").includes(SOCKET_WV));
console.assert(rootKnowledgeHandles("wv_core_laws")?.source === SOCKET_WV);
console.assert(rootKnowledgeHandles("story_rules")?.source === SOCKET_SR);
console.assert(rootKnowledgeHandles("write_prompts")?.source === SOCKET_WP);
console.assert(rootKnowledgeHandles("") == null);

const tree: NovelTree = {
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
      linked_knowledge_ids: ["hub", "core"],
      position: { x: 0, y: 0 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    },
    {
      id: "hub",
      kind: "knowledge",
      label: "生成/精修",
      outline: "",
      detailed_outline: [],
      character: null,
      knowledge: { book_ids: [], extract_prompt: "", extracted: "", slot: "write_prompts" },
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: 0, y: 0 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    },
    {
      id: "core",
      kind: "knowledge",
      label: "核心法则",
      outline: "",
      detailed_outline: [],
      character: null,
      knowledge: { book_ids: [], extract_prompt: "", extracted: "", slot: "wv_core_laws" },
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: 0, y: 0 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    },
  ],
  edges: [
    {
      id: "e1",
      source: "root",
      target: "hub",
      kind: "knowledge",
      source_handle: "left",
      target_handle: "right",
    },
    {
      id: "e2",
      source: "root",
      target: "core",
      kind: "knowledge",
      source_handle: "top",
      target_handle: "bottom",
    },
  ],
};

console.assert(migrateRootSpecialEdges(tree) === true, "migrates old handles");
console.assert(tree.edges[0]?.source_handle === SOCKET_WP);
console.assert(tree.edges[1]?.source_handle === SOCKET_WV);
console.assert(migrateRootSpecialEdges(tree) === false, "idempotent");
console.log("flowSockets.selfcheck ok");
