import type { NovelTree } from "@/lib/api";
import {
  buildKnowledgeNavItems,
  filterHostPanelLinkedKnowledge,
  isHostPanelHiddenKnowledgeSlot,
  mergeRootKnowledgeOrder,
  orderKnowledgeNodesByIds,
  sortLinkedKnowledgeNodes,
} from "./knowledgeListSort";

const t = (k: string) =>
  (
    ({
      "workspace.wv.coreLaws": "核心法则",
      "workspace.wv.spatiotemporal": "时空地理",
      "workspace.wv.socialPower": "社会权力",
      "workspace.wv.storyRules": "故事规则",
      "workspace.knowledgeCard": "知识卡",
    }) as Record<string, string>
  )[k] ?? k;

function mk(id: string, slot: string, label: string, linked: string[] = []): NovelTree["nodes"][0] {
  return {
    id,
    kind: "knowledge",
    label,
    outline: "",
    character: null,
    knowledge: { book_ids: [], extract_prompt: "", extracted: "", slot },
    side_plot: null,
    linked_character_ids: [],
    linked_side_plot_ids: [],
    linked_knowledge_ids: linked,
    position: { x: 0, y: 0 },
    word_count: 0,
    word_count_min: 0,
    word_count_max: 0,
    chapter_count: 0,
  };
}

const tree: NovelTree = {
  novel_id: "",
  nodes: [
    {
      id: "root",
      kind: "novel",
      label: "R",
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
    mk("misc", "", "杂项卡"),
    mk("rules", "story_rules", "规则"),
    mk("sp", "wv_spatiotemporal", "时空", ["loc2", "loc1"]),
    mk("loc1", "wv_location", "甲港"),
    mk("loc2", "wv_location", "乙港"),
    mk("core", "wv_core_laws", "核心", ["ax2", "ax1"]),
    mk("ax1", "wv_axiom", "公理甲"),
    mk("ax2", "wv_axiom", "公理乙"),
    mk("extra", "", "额外"),
  ],
  edges: [
    { id: "e1", source: "core", target: "ax1", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
    { id: "e2", source: "core", target: "ax2", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
    { id: "e3", source: "sp", target: "loc1", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
    { id: "e4", source: "sp", target: "loc2", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
  ],
};

const nav = buildKnowledgeNavItems(tree, t);
console.assert(nav[0]?.id === "core" && nav[0]?.indent === 0, "core first");
console.assert(nav[1]?.id === "ax2" && nav[1]?.indent === 1, "axiom linked order");
console.assert(nav[2]?.id === "ax1", "axiom linked order 2");
console.assert(nav[3]?.id === "sp", "spatiotemporal after core block");
console.assert(nav[4]?.id === "loc2" && nav[5]?.id === "loc1", "location linked order");
console.assert(
  nav.findIndex((x) => x.id === "rules") > nav.findIndex((x) => x.id === "sp"),
  "story rules after fan block",
);

console.assert(isHostPanelHiddenKnowledgeSlot("wv_core_laws"), "hide fan slot");
console.assert(isHostPanelHiddenKnowledgeSlot("story_rules"), "hide story rules");
console.assert(isHostPanelHiddenKnowledgeSlot("write_prompts"), "hide write prompts hub");
console.assert(!isHostPanelHiddenKnowledgeSlot("wv_axiom"), "show axiom in panel");

const panelNodes = filterHostPanelLinkedKnowledge(tree.nodes.filter((n) => n.kind === "knowledge"));
console.assert(!panelNodes.some((n) => n.id === "core" || n.id === "rules"), "panel filters top7");
console.assert(panelNodes.some((n) => n.id === "ax1"), "panel keeps child cards");

const sorted = sortLinkedKnowledgeNodes(panelNodes);
console.assert(!sorted.some((n) => n.id === "core"), "linked sort respects panel filter");
console.assert(sorted.some((n) => n.id === "misc"), "misc visible");

const ordered = orderKnowledgeNodesByIds(tree.nodes, ["misc", "ax1", "missing"]);
console.assert(ordered.map((n) => n.id).join(",") === "misc,ax1", "orderKnowledgeNodesByIds");

const merged = mergeRootKnowledgeOrder(
  tree.nodes,
  ["misc", "core", "rules", "extra"],
  ["extra", "misc"],
);
console.assert(
  JSON.stringify(merged) === JSON.stringify(["core", "rules", "extra", "misc"]),
  "fixed slots stay front when reordering root",
);

console.log("knowledgeListSort.selfcheck ok");
