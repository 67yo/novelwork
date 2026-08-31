import { applyWorldviewPayload } from "./worldviewGen.ts";
import { ensureWorldviewCards, knowledgeSlot } from "./worldview.ts";
import type { NovelTree } from "./api.ts";

const t = (k: string) =>
  ({
    "workspace.wv.coreLaws": "核心法则",
    "workspace.wv.spatiotemporal": "时空地理",
    "workspace.wv.socialPower": "社会权力",
    "workspace.wv.historyCulture": "历史文化",
    "workspace.wv.existence": "存在基础",
    "workspace.wv.infoFlow": "信息传播",
    "workspace.wv.storyRules": "故事规则",
  })[k] ?? k;

const tree: NovelTree = {
  novel_id: "",
  nodes: [
    {
      id: "root",
      kind: "novel",
      label: "书",
      outline: "蒸汽朋克帝国",
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

ensureWorldviewCards(tree, t);
const ok = applyWorldviewPayload(
  tree,
  {
    core_laws: {
      premise: "齿轮与魔法共存",
      taboos: ["禁止改写历史"],
      power_system: "蒸汽核心",
      power_expression: "机械义肢",
      axioms: [
        {
          name: "能量守恒",
          statement: "魔法必消耗煤",
          boundary: "不创造物质",
          cost: "燃料",
          mechanism: "锅炉",
        },
      ],
    },
    existence: { premise: "凡人可修行", death: "魂归熔炉", calendar: "帝国历", lifespan: "80", disease_reproduction: "瘟疫偶发" },
    history_culture: {
      premise: "帝国由蒸汽革命重塑",
      customs: "节庆祭炉",
      economy: "专利与行会",
      daily_slices: "夜班工厂与早市",
      religions: [{ name: "炉神教", core_belief: "火焰净化", followers_scope: "工匠" }],
      major_events: [{ title: "铁冠条约", event: "议会夺权", long_term_impact: "寡头长期执政" }],
    },
    story_rules: { extracted: "第三人称；每章留悬念" },
  },
  t,
);

console.assert(ok, "applyWorldviewPayload");
console.assert(
  tree.nodes.some((n) => knowledgeSlot(n) === "wv_axiom"),
  "axiom child",
);
console.assert(
  tree.nodes.find((n) => knowledgeSlot(n) === "wv_core_laws")?.knowledge?.extracted.includes("能量守恒"),
  "core laws extracted",
);
console.assert(
  tree.nodes.some((n) => knowledgeSlot(n) === "wv_religion"),
  "religion child",
);
console.assert(
  tree.nodes.find((n) => knowledgeSlot(n) === "wv_history_culture")?.knowledge?.extracted.includes("炉神教"),
  "history culture extracted",
);

const coreBefore = tree.nodes.find((n) => knowledgeSlot(n) === "wv_core_laws")?.knowledge?.extracted ?? "";
const onlyExistence = applyWorldviewPayload(
  tree,
  { existence: { premise: "只改存在基础", death: "魂散", calendar: "历", lifespan: "1", disease_reproduction: "无" } },
  t,
);
console.assert(onlyExistence, "slot-only apply");
console.assert(
  (tree.nodes.find((n) => knowledgeSlot(n) === "wv_core_laws")?.knowledge?.extracted ?? "") === coreBefore,
  "slot-only must not wipe other cards",
);
console.assert(
  tree.nodes.find((n) => knowledgeSlot(n) === "wv_existence")?.knowledge?.extracted.includes("只改存在基础"),
  "existence filled",
);
console.log("worldviewGen.selfcheck ok");
