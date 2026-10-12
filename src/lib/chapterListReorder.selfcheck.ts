/** ponytail: manuscript chapter reorder. Run: npx tsx src/lib/chapterListReorder.selfcheck.ts */
import {
  FREE_CHAPTER_GROUP,
  assignChapterYs,
  manuscriptGroups,
  relinkChapterChain,
  reorderInGroup,
} from "./chapterListReorder.ts";

const groups = manuscriptGroups([
  { id: "v1", kind: "volume", indent: 0 },
  { id: "c1", kind: "chapter", indent: 1 },
  { id: "c2", kind: "chapter", indent: 1 },
  { id: "c3", kind: "chapter", indent: 1 },
  { id: "c4", kind: "chapter", indent: 0 },
]);
if (groups.get("v1")?.join() !== "c1,c2,c3") throw new Error("volume group");
if (groups.get(FREE_CHAPTER_GROUP)?.join() !== "c4") throw new Error("free group");
const twoVol = manuscriptGroups([
  { id: "v1", kind: "volume", indent: 0 },
  { id: "a", kind: "chapter", indent: 1 },
  { id: "v2", kind: "volume", indent: 0 },
  { id: "b", kind: "chapter", indent: 1 },
]);
if (twoVol.get("v1")?.join() !== "a" || twoVol.get("v2")?.join() !== "b") {
  throw new Error("split volumes");
}

const moved = reorderInGroup(["c1", "c2", "c3"], "c3", "c1");
if (moved?.join() !== "c3,c1,c2") throw new Error(`want c3,c1,c2 got ${moved}`);
if (reorderInGroup(["c1", "c2"], "c1", "c1") != null) throw new Error("same index");
if (reorderInGroup(["c1"], "x", "c1") != null) throw new Error("missing");

const nodes = [
  { id: "c1", position: { x: 0, y: 10 } },
  { id: "c2", position: { x: 0, y: 20 } },
  { id: "c3", position: { x: 0, y: 30 } },
];
assignChapterYs(nodes, ["c3", "c1", "c2"]);
if (nodes.map((n) => `${n.id}:${n.position.y}`).join() !== "c1:20,c2:30,c3:10") {
  throw new Error("ys follow new order");
}
const tied = [
  { id: "a", position: { x: 0, y: 5 } },
  { id: "b", position: { x: 0, y: 5 } },
];
assignChapterYs(tied, ["b", "a"]);
if (!(tied[1]!.position.y < tied[0]!.position.y)) throw new Error("tied ys must split");

const edges = relinkChapterChain(
  [
    { id: "keep", source: "root", target: "v1", kind: "volume" },
    { id: "old", source: "v1", target: "c1", kind: "chapter" },
    { id: "old2", source: "c1", target: "c2", kind: "chapter" },
    { id: "char", source: "v1", target: "hero", kind: "character" },
  ],
  "v1",
  ["c2", "c1"],
);
if (!edges.some((e) => e.id === "keep" && e.kind === "volume")) throw new Error("keep volume edge");
if (!edges.some((e) => e.kind === "character" && e.target === "hero")) throw new Error("keep character");
if (edges.some((e) => e.id === "old" || e.id === "old2")) throw new Error("drop old chain");
if (!edges.some((e) => e.source === "v1" && e.target === "c2" && e.kind === "chapter")) {
  throw new Error("host → first");
}
if (!edges.some((e) => e.source === "c2" && e.target === "c1")) throw new Error("c2 → c1");

console.log("chapterListReorder.selfcheck ok");
