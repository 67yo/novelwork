/** ponytail: line LCS diff. Run: npx tsx src/lib/linediff.selfcheck.ts */
import { applyDiffPick, diffLines, groupDiffHunks } from "./linediff.ts";

const same = diffLines("a\nb", "a\nb");
console.assert(same.every((h) => h.type === "eq") && same.length === 2, "eq");

const add = diffLines("", "hello");
console.assert(add.some((h) => h.type === "add" && h.text === "hello"), "add");

const del = diffLines("gone", "");
console.assert(del.some((h) => h.type === "del" && h.text === "gone"), "del");

const mixed = diffLines("keep\nold\nend", "keep\nnew\nend");
const types = mixed.map((h) => `${h.type}:${h.text}`).join("|");
console.assert(types.includes("eq:keep") && types.includes("del:old") && types.includes("add:new") && types.includes("eq:end"), types);

const grouped = groupDiffHunks(mixed);
const conflictAt = grouped.findIndex((b) => b.type === "conflict");
if (conflictAt < 0) throw new Error("expected conflict");
const keepOld = applyDiffPick(grouped, conflictAt, "old");
if (keepOld.before !== "keep\nold\nend" || keepOld.after !== "keep\nold\nend") {
  throw new Error(`keep old: ${JSON.stringify(keepOld)}`);
}
const keepNew = applyDiffPick(grouped, conflictAt, "new");
if (keepNew.before !== "keep\nnew\nend" || keepNew.after !== "keep\nnew\nend") {
  throw new Error(`keep new: ${JSON.stringify(keepNew)}`);
}
if (diffLines(keepNew.before, keepNew.after).some((h) => h.type !== "eq")) {
  throw new Error("picked region should no longer differ");
}

const two = groupDiffHunks(diffLines("a\nx\nb\ny\nc", "a\nX\nb\nY\nc"));
const conflicts = two.flatMap((b, i) => (b.type === "conflict" ? [i] : []));
if (conflicts.length !== 2) throw new Error(`expected 2 conflicts, got ${conflicts.length}`);
const firstOld = applyDiffPick(two, conflicts[0]!, "old");
if (firstOld.after !== "a\nx\nb\nY\nc") throw new Error(`first old: ${firstOld.after}`);

console.log("linediff.selfcheck ok");
