import type { NovelTree } from "@/lib/api";
import {
  buildBookNavItems,
  buildCharacterNavItems,
  buildPlotNavItems,
  buildStoryRulesNavItems,
  buildKnowledgeNavItems,
  buildWorldviewNavItems,
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
      "workspace.sr.surface": "表层设定",
      "workspace.sr.engine": "故事引擎",
      "workspace.sr.fulfillment": "兑现系统",
      "workspace.sr.constraints": "约束红线",
      "workspace.knowledgeCard": "知识卡",
      "workspace.writePrompts.title": "生成/精修",
      "workspace.writePrompts.handleGenerate": "生成用知识",
      "workspace.writePrompts.handleRefine": "精修用知识",
      "workspace.bookSelectRoot": "封面与规划",
      "workspace.rootLinkedKnowledge": "关联知识卡",
      "workspace.coreLaws.axioms": "世界公理",
      "workspace.st.locations": "关键地点",
      "workspace.pow.races": "种族",
      "workspace.pow.factions": "主要势力",
      "workspace.tabBook": "全书",
      "workspace.volumeBadge": "分卷",
      "workspace.newChapter": "新章节",
      "workspace.newCharacter": "新人物",
      "workspace.plot": "剧情",
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

const wvNav = buildWorldviewNavItems(tree, t);
console.assert(wvNav[0]?.role === "parent" && wvNav[0]?.id === "core", "wv parent");
console.assert(wvNav[1]?.role === "group" && wvNav[1]?.label === "世界公理", "wv axiom group");
console.assert(wvNav[2]?.role === "child" && wvNav[2]?.id === "ax2", "wv axiom child");
console.assert(!wvNav.some((x) => x.id === "rules"), "wv nav excludes story rules");

const powTree: NovelTree = {
  ...tree,
  nodes: [
    ...tree.nodes,
    mk("pow", "wv_social_power", "权力", ["r1", "f1"]),
    mk("r1", "wv_race", "精灵"),
    mk("f1", "wv_faction", "帝国"),
  ],
  edges: [
    ...tree.edges,
    { id: "e5", source: "pow", target: "r1", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
    { id: "e6", source: "pow", target: "f1", kind: "knowledge", source_handle: "top", target_handle: "bottom" },
  ],
};
const powNav = buildWorldviewNavItems(powTree, t);
const raceG = powNav.find((x) => x.role === "group" && x.label === "种族");
const factG = powNav.find((x) => x.role === "group" && x.label === "主要势力");
console.assert(raceG?.tone === "rose" && factG?.tone === "indigo", "social power groups split");
console.assert(powNav.some((x) => x.id === "r1" && x.tone === "rose"), "race child tone");
console.assert(powNav.some((x) => x.id === "f1" && x.tone === "indigo"), "faction child tone");

const bookTree: NovelTree = {
  novel_id: "",
  nodes: [
    {
      ...tree.nodes[0]!,
      id: "root",
      kind: "novel",
      linked_knowledge_ids: ["wp", "misc"],
    },
    mk("wp", "write_prompts", "生成/精修", ["g1", "r1"]),
    mk("g1", "", "生成卡"),
    mk("r1", "", "精修卡"),
    mk("misc", "", "杂项卡"),
  ],
  edges: [
    {
      id: "e-wp-g",
      source: "wp",
      target: "g1",
      kind: "knowledge",
      source_handle: "left",
      target_handle: "right",
    },
    {
      id: "e-wp-r",
      source: "wp",
      target: "r1",
      kind: "knowledge",
      source_handle: "right",
      target_handle: "right",
    },
    {
      id: "e-root-misc",
      source: "root",
      target: "misc",
      kind: "knowledge",
      source_handle: "right",
      target_handle: "right",
    },
  ],
};
const bookNav = buildBookNavItems(bookTree, t);
console.assert(bookNav[0]?.role === "parent" && bookNav[0]?.id === "root", "book cover parent");
console.assert(bookNav[0]?.label === "封面与规划", "book cover label");
console.assert(bookNav.some((x) => x.role === "parent" && x.id === "wp"), "book write hub");
console.assert(
  bookNav.some((x) => x.role === "group" && x.label === "生成用知识"),
  "book generate group",
);
console.assert(
  bookNav.some((x) => x.role === "group" && x.label === "精修用知识" && x.tone === "indigo"),
  "book refine group",
);
console.assert(bookNav.some((x) => x.id === "g1" && x.tone === "teal"), "book generate child");
console.assert(bookNav.some((x) => x.id === "r1" && x.tone === "indigo"), "book refine child");
console.assert(
  bookNav.some((x) => x.role === "group" && x.label === "关联知识卡"),
  "book generic group",
);
console.assert(bookNav.some((x) => x.id === "misc" && x.role === "child"), "book generic child");

function mkChar(id: string, label: string): NovelTree["nodes"][0] {
  return {
    id,
    kind: "character",
    label,
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
  };
}

const charTree: NovelTree = {
  novel_id: "",
  nodes: [
    { ...tree.nodes[0]!, linked_character_ids: ["c2", "c1"] },
    {
      ...tree.nodes[0]!,
      id: "vol1",
      kind: "volume",
      label: "上卷",
      linked_character_ids: ["c3"],
      linked_knowledge_ids: [],
      position: { x: 0, y: 10 },
    },
    mkChar("c1", "甲"),
    mkChar("c2", "乙"),
    mkChar("c3", "丙"),
    mkChar("c4", "孤"),
  ],
  edges: [],
};
const charNav = buildCharacterNavItems(charTree, t);
console.assert(charNav[0]?.role === "group" && charNav[0]?.label === "全书", "char root group");
console.assert(charNav[1]?.id === "c2" && charNav[2]?.id === "c1", "char root linked order");
console.assert(
  charNav.some((x) => x.role === "group" && x.label === "上卷" && x.tone === "amber"),
  "char volume group",
);
console.assert(charNav.some((x) => x.id === "c3" && x.tone === "amber"), "char volume child");
console.assert(charNav.some((x) => x.id === "c4" && x.role === "parent"), "char orphan parent");

function mkPlot(id: string, label: string): NovelTree["nodes"][0] {
  return {
    id,
    kind: "side_plot",
    label,
    outline: "",
    character: null,
    knowledge: null,
    side_plot: { status: "active", absorbed: false },
    linked_character_ids: [],
    linked_side_plot_ids: [],
    linked_knowledge_ids: [],
    position: { x: 0, y: 0 },
    word_count: 0,
    word_count_min: 0,
    word_count_max: 0,
    chapter_count: 0,
  };
}

const plotTree: NovelTree = {
  novel_id: "",
  nodes: [
    { ...tree.nodes[0]!, linked_side_plot_ids: ["p2", "p1"] },
    {
      ...tree.nodes[0]!,
      id: "vol1",
      kind: "volume",
      label: "上卷",
      linked_side_plot_ids: ["p3"],
      linked_knowledge_ids: [],
      position: { x: 0, y: 10 },
    },
    {
      ...tree.nodes[0]!,
      id: "ch1",
      kind: "chapter",
      label: "第一章",
      linked_side_plot_ids: ["p5"],
      linked_knowledge_ids: [],
      position: { x: 0, y: 20 },
    },
    mkPlot("p1", "甲线"),
    mkPlot("p2", "乙线"),
    mkPlot("p3", "卷线"),
    mkPlot("p4", "孤"),
    mkPlot("p5", "章线"),
  ],
  edges: [],
};
const plotNav = buildPlotNavItems(plotTree, t);
console.assert(plotNav[0]?.role === "group" && plotNav[0]?.label === "全书", "plot root group");
console.assert(plotNav[1]?.id === "p2" && plotNav[2]?.id === "p1", "plot root linked order");
console.assert(
  plotNav.some((x) => x.role === "group" && x.label === "上卷" && x.tone === "sky"),
  "plot volume group",
);
console.assert(plotNav.some((x) => x.id === "p3" && x.tone === "sky"), "plot volume child");
console.assert(
  plotNav.some((x) => x.role === "group" && x.label === "第一章" && x.tone === "sky"),
  "plot chapter group",
);
console.assert(plotNav.some((x) => x.id === "p5" && x.tone === "sky"), "plot chapter child");
console.assert(plotNav.some((x) => x.id === "p4" && x.role === "parent"), "plot orphan parent");

const srTree: NovelTree = {
  novel_id: "",
  nodes: [
    tree.nodes[0]!,
    mk("rules", "story_rules", "规则", ["eng", "surf", "ful", "red"]),
    mk("surf", "sr_surface_setting", "表层"),
    mk("eng", "sr_story_engine", "引擎"),
    mk("ful", "sr_fulfillment_system", "兑现"),
    mk("red", "sr_constraint_redlines", "红线"),
  ],
  edges: [
    { id: "e-r-s", source: "rules", target: "surf", kind: "knowledge" },
    { id: "e-r-e", source: "rules", target: "eng", kind: "knowledge" },
    { id: "e-r-f", source: "rules", target: "ful", kind: "knowledge" },
    { id: "e-r-r", source: "rules", target: "red", kind: "knowledge" },
  ],
};
const srNav = buildStoryRulesNavItems(srTree, t);
console.assert(srNav[0]?.role === "parent" && srNav[0]?.id === "rules", "sr hub parent");
console.assert(srNav[0]?.label === "故事规则", "sr hub label");
console.assert(
  srNav.slice(1).map((x) => x.id).join(",") === "surf,eng,ful,red",
  "sr blocks follow fan slot order",
);
console.assert(srNav[1]?.role === "child" && srNav[1]?.label === "表层设定", "sr surface child");

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
