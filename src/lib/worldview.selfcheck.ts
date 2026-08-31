import { ensureWorldviewCards, missingWorldviewSlots, worldviewComplete, worldviewJsonKeyForSlot } from "./worldview.ts";
import type { NovelTree } from "./api.ts";

const titles: Record<string, string> = {
  "workspace.wv.coreLaws": "核心法则",
  "workspace.wv.spatiotemporal": "时空地理",
  "workspace.wv.socialPower": "社会权力",
  "workspace.wv.historyCulture": "历史文化",
  "workspace.wv.existence": "存在基础",
  "workspace.wv.infoFlow": "信息传播",
  "workspace.wv.storyRules": "故事规则",
};

const tree: NovelTree = {
  novel_id: "n1",
  nodes: [
    {
      id: "root",
      kind: "novel",
      label: "书",
      outline: "",
      character: null,
      knowledge: null,
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
  edges: [],
};

console.assert(missingWorldviewSlots(tree).length === 7);
console.assert(ensureWorldviewCards(tree, (k) => titles[k] ?? k) === true);
console.assert(worldviewComplete(tree));
console.assert(tree.nodes.filter((n) => n.kind === "knowledge").length === 7);
console.assert(ensureWorldviewCards(tree, (k) => titles[k] ?? k) === false);
const fan = tree.edges.find((e) => e.target === tree.nodes.find((n) => n.knowledge?.slot === "wv_core_laws")!.id);
console.assert(fan?.source_handle === "top" && fan?.target_handle === "bottom");
const rules = tree.edges.find((e) => e.target === tree.nodes.find((n) => n.knowledge?.slot === "story_rules")!.id);
console.assert(rules?.source_handle === "right" && rules?.target_handle === "left");
console.assert(worldviewJsonKeyForSlot("wv_existence") === "existence");
console.assert(worldviewJsonKeyForSlot("core_laws") === "core_laws");
console.assert(worldviewJsonKeyForSlot("story_rules") === null);
console.log("worldview.selfcheck ok");
