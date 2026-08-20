import {
  applyAutoLayout,
  chapterEffectiveKnowledgeIds,
  chapterLocalPlotIds,
  rootLinkedPlotIds,
  __layoutConsts as C,
} from "./treeLayout.ts";

const nodes = [
  {
    id: "r",
    kind: "novel",
    position: { x: 0, y: 0 },
    linked_side_plot_ids: [] as string[],
    linked_character_ids: ["a_root"],
    linked_knowledge_ids: ["k_root"],
  },
  {
    id: "c1",
    kind: "chapter",
    position: { x: 0, y: 10 },
    linked_side_plot_ids: ["p1", "p2", "p3", "p4", "p5", "pm"],
    linked_character_ids: ["a1", "a2", "a3", "a4", "a5"],
    linked_knowledge_ids: ["k1"],
  },
  {
    id: "c2",
    kind: "chapter",
    position: { x: 0, y: 20 },
    linked_side_plot_ids: ["pm"],
    linked_character_ids: [] as string[],
    linked_knowledge_ids: [] as string[],
  },
  ...[1, 2, 3, 4, 5].map((i) => ({
    id: `p${i}`,
    kind: "side_plot",
    position: { x: 0, y: i },
    linked_side_plot_ids: [] as string[],
  })),
  {
    id: "pm",
    kind: "side_plot",
    position: { x: 0, y: 0 },
    linked_side_plot_ids: [] as string[],
  },
  ...[1, 2, 3, 4, 5].map((i) => ({
    id: `a${i}`,
    kind: "character",
    position: { x: 0, y: i },
    linked_side_plot_ids: [] as string[],
  })),
  { id: "a_root", kind: "character", position: { x: 0, y: 0 }, linked_side_plot_ids: [] as string[] },
  { id: "k_root", kind: "knowledge", position: { x: 0, y: 0 }, linked_side_plot_ids: [] as string[] },
  { id: "k1", kind: "knowledge", position: { x: 0, y: 99 }, linked_side_plot_ids: [] as string[] },
];
applyAutoLayout(nodes, []);
const get = (id: string) => nodes.find((n) => n.id === id)!;

console.assert(get("r").position.x === C.MAIN_X);
console.assert(get("r").position.y === C.TOP);
console.assert(get("a1").position.x === C.CHAR_X);
console.assert(get("c1").position.x === C.MAIN_X);
console.assert(get("p1").position.x === C.PLOT_X);
// 同章竖向紧凑
console.assert(get("p2").position.y - get("p1").position.y === C.PLOT_DY);
// 第 5 张横排到下一列、回到顶行
console.assert(get("p5").position.x === C.PLOT_X + C.PLOT_DX);
console.assert(get("p5").position.y === get("p1").position.y);
// 人物卡同章竖排，第 5 张向左横排
console.assert(get("a5").position.x === C.CHAR_X - C.SIDE_DX);
console.assert(get("a5").position.y === get("a1").position.y);
console.assert(get("a2").position.y > get("a1").position.y);
// 知识卡在人物左侧
const leftCharX = Math.min(
  get("a1").position.x,
  get("a2").position.x,
  get("a3").position.x,
  get("a4").position.x,
  get("a5").position.x,
);
console.assert(get("k1").position.x === leftCharX - C.KNOW_GAP_FROM_CHAR);
console.assert(get("k1").position.y === get("c1").position.y);
// 根上侧卡与根同 y（主轴首项）
console.assert(get("a_root").position.x === C.CHAR_X);
console.assert(get("a_root").position.y === get("r").position.y);
console.assert(get("k_root").position.x === C.CHAR_X - C.KNOW_GAP_FROM_CHAR);
console.assert(get("k_root").position.y === get("r").position.y);
// 根有链接 → 首章按大间距拉开
console.assert(get("c1").position.y - get("r").position.y >= C.MIN_CHAPTER_GAP);

// 实测高度：高人物卡应拉开下一张
{
  const tall = [
    {
      id: "r2",
      kind: "novel",
      position: { x: 0, y: 0 },
      linked_character_ids: ["t1", "t2"],
      linked_side_plot_ids: [] as string[],
      linked_knowledge_ids: [] as string[],
    },
    {
      id: "t1",
      kind: "character",
      position: { x: 0, y: 0 },
      linked_side_plot_ids: [] as string[],
    },
    {
      id: "t2",
      kind: "character",
      position: { x: 0, y: 1 },
      linked_side_plot_ids: [] as string[],
    },
  ];
  const sizes = new Map([
    ["t1", { w: 160, h: 140 }],
    ["t2", { w: 160, h: 80 }],
  ]);
  applyAutoLayout(tall, [], sizes);
  const t1 = tall.find((n) => n.id === "t1")!;
  const t2 = tall.find((n) => n.id === "t2")!;
  console.assert(t2.position.y === t1.position.y + 140 + C.SIDE_GAP);
}
// 有链接章：下章远离上章，给 4 行剧情 / 左侧块腾位
const need = C.MAX_PLOT_COL * C.PLOT_DY + C.CH_PAD;
console.assert(get("c2").position.y - get("c1").position.y >= need);

// 多章共用剧情：落在两章垂直中点
const midY = (get("c1").position.y + get("c2").position.y) / 2;
console.assert(Math.abs(get("pm").position.y - midY) < 1e-6);
const rightmostSingle = Math.max(get("p1").position.x, get("p5").position.x);
const expectMultiX = rightmostSingle + C.PLOT_CARD_W + C.PLOT_CARD_W / 2;
console.assert(Math.abs(get("pm").position.x - expectMultiX) < 1e-6);

// 无链接章节：主轴竖向贴近
{
  const bare = [
    { id: "br", kind: "novel", position: { x: 0, y: 0 } },
    { id: "b1", kind: "chapter", position: { x: 0, y: 10 } },
    { id: "b2", kind: "chapter", position: { x: 0, y: 20 } },
    { id: "b3", kind: "chapter", position: { x: 0, y: 30 } },
  ];
  applyAutoLayout(bare, []);
  const g = (id: string) => bare.find((n) => n.id === id)!;
  console.assert(g("br").position.y === C.TOP);
  const d12 = g("b2").position.y - g("b1").position.y;
  const d23 = g("b3").position.y - g("b2").position.y;
  console.assert(d12 < C.MIN_CHAPTER_GAP, `bare gap ${d12}`);
  console.assert(d12 >= C.MIN_CHAPTER_GAP_TIGHT, `bare gap floor ${d12}`);
  console.assert(d23 === d12);
  // 根也无链接 → 根到首章同样贴近
  console.assert(g("b1").position.y - g("br").position.y < C.MIN_CHAPTER_GAP);
}

// 剧情卡顺序跟章节 linked_side_plot_ids（与旧 y 无关）
{
  const ordered = [
    {
      id: "oc",
      kind: "chapter",
      position: { x: 0, y: 0 },
      linked_side_plot_ids: ["ob", "oa"],
    },
    {
      id: "oa",
      kind: "side_plot",
      position: { x: 0, y: 0 },
      linked_side_plot_ids: [] as string[],
    },
    {
      id: "ob",
      kind: "side_plot",
      position: { x: 0, y: 50 },
      linked_side_plot_ids: [] as string[],
    },
  ];
  applyAutoLayout(ordered, []);
  const oa = ordered.find((n) => n.id === "oa")!;
  const ob = ordered.find((n) => n.id === "ob")!;
  const oc = ordered.find((n) => n.id === "oc")!;
  console.assert(ob.position.y === oc.position.y, "order0 plot aligns with chapter");
  console.assert(oa.position.y === ob.position.y + C.PLOT_DY, "order1 below order0");
}

{
  const nodes = [
    {
      id: "root",
      kind: "novel",
      position: { x: 0, y: 0 },
      linked_side_plot_ids: ["rp"],
      linked_knowledge_ids: ["rk"],
    },
    {
      id: "ch",
      kind: "chapter",
      position: { x: 0, y: 10 },
      linked_side_plot_ids: ["rp", "cp"],
      linked_knowledge_ids: ["ck"],
    },
    { id: "rp", kind: "side_plot", position: { x: 0, y: 0 } },
    { id: "cp", kind: "side_plot", position: { x: 0, y: 1 } },
    { id: "rk", kind: "knowledge", position: { x: 0, y: 0 } },
    { id: "ck", kind: "knowledge", position: { x: 0, y: 1 } },
  ];
  console.assert(
    JSON.stringify(rootLinkedPlotIds(nodes, [])) === JSON.stringify(["rp"]),
    "root plots",
  );
  console.assert(
    JSON.stringify(chapterLocalPlotIds("ch", nodes, [])) === JSON.stringify(["cp"]),
    "chapter local plots exclude root",
  );
  console.assert(
    JSON.stringify(chapterEffectiveKnowledgeIds("ch", nodes, [])) ===
      JSON.stringify(["ck", "rk"]),
    "chapter knowledge merges root",
  );
}

{
  const nodes = [
    {
      id: "root",
      kind: "novel",
      position: { x: 0, y: 0 },
      linked_side_plot_ids: ["rp"],
      linked_knowledge_ids: ["rk"],
      linked_character_ids: [] as string[],
    },
    {
      id: "vol",
      kind: "volume",
      position: { x: 0, y: 5 },
      linked_side_plot_ids: ["vp"],
      linked_knowledge_ids: ["vk"],
      linked_character_ids: [] as string[],
    },
    {
      id: "ch",
      kind: "chapter",
      position: { x: 0, y: 10 },
      linked_side_plot_ids: ["cp"],
      linked_knowledge_ids: ["ck"],
      linked_character_ids: [] as string[],
    },
    { id: "rp", kind: "side_plot", position: { x: 0, y: 0 } },
    { id: "vp", kind: "side_plot", position: { x: 0, y: 1 } },
    { id: "cp", kind: "side_plot", position: { x: 0, y: 2 } },
    { id: "rk", kind: "knowledge", position: { x: 0, y: 0 } },
    { id: "vk", kind: "knowledge", position: { x: 0, y: 1 } },
    { id: "ck", kind: "knowledge", position: { x: 0, y: 2 } },
  ];
  const edges = [
    { id: "e1", source: "root", target: "vol", kind: "volume" },
    { id: "e2", source: "vol", target: "ch", kind: "chapter" },
  ];
  applyAutoLayout(nodes, edges);
  console.assert(nodes.find((n) => n.id === "vol")!.position.x === C.MAIN_X, "volume on spine");
  console.assert(
    JSON.stringify(chapterLocalPlotIds("ch", nodes, edges)) === JSON.stringify(["cp"]),
    "volume inherit excluded from local plots",
  );
  console.assert(
    JSON.stringify(chapterEffectiveKnowledgeIds("ch", nodes, edges)).includes("vk") &&
      JSON.stringify(chapterEffectiveKnowledgeIds("ch", nodes, edges)).includes("rk"),
    "chapter knowledge merges root+volume",
  );
}

{
  // 分卷主轴：root → vol1 → ch_a → ch_b → vol2 → ch_c（卷下章节聚在卷后，不被全局 y 打散）
  const nodes = [
    { id: "r", kind: "novel", position: { x: 0, y: 0 } },
    { id: "v2", kind: "volume", position: { x: 0, y: 1 } }, // 故意 y 更靠上
    { id: "v1", kind: "volume", position: { x: 0, y: 50 } },
    { id: "cb", kind: "chapter", position: { x: 0, y: 2 } },
    { id: "ca", kind: "chapter", position: { x: 0, y: 3 } },
    { id: "cc", kind: "chapter", position: { x: 0, y: 4 } },
  ];
  const edges = [
    { source: "r", target: "v1", kind: "volume" },
    { source: "v1", target: "v2", kind: "volume" },
    { source: "v1", target: "ca", kind: "chapter" },
    { source: "ca", target: "cb", kind: "chapter" },
    { source: "v2", target: "cc", kind: "chapter" },
  ];
  applyAutoLayout(nodes, edges);
  const y = (id: string) => nodes.find((n) => n.id === id)!.position.y;
  console.assert(y("r") < y("v1"), "root above vol1");
  console.assert(y("v1") < y("ca"), "vol1 above its chapters");
  console.assert(y("ca") < y("cb"), "chapter chain under vol1");
  console.assert(y("cb") < y("v2"), "vol1 block finishes before vol2");
  console.assert(y("v2") < y("cc"), "vol2 above its chapter");
  // 即使 v2 初始 y 更小，结构序仍把 v1 放在 v2 前（从根挂出的链）
  console.assert(y("v1") < y("v2"), "volume chain from root");
}

console.log("treeLayout.selfcheck ok");
