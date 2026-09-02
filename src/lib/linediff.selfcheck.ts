/** ponytail: line LCS diff. Run: npx tsx src/lib/linediff.selfcheck.ts */
import { diffLines } from "./linediff.ts";

const same = diffLines("a\nb", "a\nb");
console.assert(same.every((h) => h.type === "eq") && same.length === 2, "eq");

const add = diffLines("", "hello");
console.assert(add.some((h) => h.type === "add" && h.text === "hello"), "add");

const del = diffLines("gone", "");
console.assert(del.some((h) => h.type === "del" && h.text === "gone"), "del");

const mixed = diffLines("keep\nold\nend", "keep\nnew\nend");
const types = mixed.map((h) => `${h.type}:${h.text}`).join("|");
console.assert(types.includes("eq:keep") && types.includes("del:old") && types.includes("add:new") && types.includes("eq:end"), types);

console.log("linediff.selfcheck ok");
