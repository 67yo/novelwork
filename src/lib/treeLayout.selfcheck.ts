import { applyAutoLayout, __layoutConsts as C } from "./treeLayout.ts";

const nodes = [
  { id: "r", kind: "novel", position: { x: 0, y: 0 }, linked_side_plot_ids: [] as string[] },
  {
    id: "c1",
    kind: "chapter",
    position: { x: 0, y: 10 },
    linked_side_plot_ids: ["p1", "p2", "p3", "p4", "p5"],
  },
  { id: "c2", kind: "chapter", position: { x: 0, y: 20 }, linked_side_plot_ids: [] as string[] },
  ...[1, 2, 3, 4, 5].map((i) => ({
    id: `p${i}`,
    kind: "side_plot",
    position: { x: 0, y: i },
    linked_side_plot_ids: [] as string[],
  })),
  { id: "a1", kind: "character", position: { x: 0, y: 0 }, linked_side_plot_ids: [] as string[] },
];
applyAutoLayout(nodes, []);
const get = (id: string) => nodes.find((n) => n.id === id)!;

console.assert(get("a1").position.x === C.CHAR_X);
console.assert(get("c1").position.x === C.MAIN_X);
console.assert(get("p1").position.x === C.PLOT_X);
// 同章竖向紧凑
console.assert(get("p2").position.y - get("p1").position.y === C.PLOT_DY);
// 第 5 张横排到下一列、回到顶行
console.assert(get("p5").position.x === C.PLOT_X + C.PLOT_DX);
console.assert(get("p5").position.y === get("p1").position.y);
// 下章远离上章，给 4 行剧情腾位
const need = C.MAX_PLOT_COL * C.PLOT_DY + C.CH_PAD;
console.assert(get("c2").position.y - get("c1").position.y >= need);
console.log("treeLayout.selfcheck ok");
