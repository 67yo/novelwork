/** ponytail: assert list reorder splice (chapter linked plots). Run: npx tsx src/lib/plotReorder.selfcheck.ts */
function reorderIds(ids: string[], from: number, to: number): string[] {
  const next = [...ids];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

const a = reorderIds(["p0", "p1", "p2"], 2, 0);
if (a.join() !== "p2,p0,p1") throw new Error(`want p2,p0,p1 got ${a}`);
const b = reorderIds(["p0", "p1"], 0, 1);
if (b.join() !== "p1,p0") throw new Error(`want p1,p0 got ${b}`);
console.log("plotReorder.selfcheck ok");
