/** ponytail: chapter mouse select. Run: npx tsx src/lib/chapterMouseSelect.selfcheck.ts */
import { clauseOffsets, reliableClickCount, resetClickCount } from "./chapterMouseSelect.ts";

const line = "他推开门。屋里很暗，灯还没亮。";
const [a, b] = clauseOffsets(line, 2);
console.assert(line.slice(a, b) === "他推开门", line.slice(a, b));
const [c, d] = clauseOffsets(line, 8);
console.assert(line.slice(c, d) === "屋里很暗", line.slice(c, d));
const [e, f] = clauseOffsets("hello world", 1);
console.assert("hello world".slice(e, f) === "hello", "ascii word");
console.assert(clauseOffsets("", 0).join() === "0,0");

resetClickCount();
const ev = (x: number, y: number) =>
  ({ clientX: x, clientY: y } as MouseEvent);
console.assert(reliableClickCount(ev(10, 10)) === 1, "first");
console.assert(reliableClickCount(ev(10, 10)) === 2, "second");
console.assert(reliableClickCount(ev(10, 10)) === 3, "third");
console.assert(reliableClickCount(ev(10, 10)) === 1, "wrap");
resetClickCount();
reliableClickCount(ev(0, 0));
console.assert(reliableClickCount(ev(20, 0)) === 1, "far is new click");
resetClickCount();
reliableClickCount(ev(10, 10));
resetClickCount();
console.assert(reliableClickCount(ev(10, 10)) === 1, "reset breaks streak");

console.log("chapterMouseSelect.selfcheck ok");
