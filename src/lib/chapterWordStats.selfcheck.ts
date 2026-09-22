/** ponytail: chapter range + word count. Run: npx tsx src/lib/chapterWordStats.selfcheck.ts */
import { chapterSpan, countWords } from "./chapterWordStats";

console.assert(countWords("a b\n c") === 3, "ascii skip whitespace");
console.assert(countWords("你好 世界") === 4, "cjk skip space");
console.assert(countWords("") === 0, "empty");

console.assert(JSON.stringify(chapterSpan(0, 1, 3)) === "[0,0]", "empty list");
console.assert(JSON.stringify(chapterSpan(10, null, null)) === "[0,10]", "all");
console.assert(JSON.stringify(chapterSpan(10, 3, 5)) === "[2,5]", "3-5");
console.assert(JSON.stringify(chapterSpan(10, 5, 3)) === "[2,5]", "swap");
console.assert(JSON.stringify(chapterSpan(10, 99, 99)) === "[9,10]", "clamp last");
console.assert(JSON.stringify(chapterSpan(10, 0, 2)) === "[0,2]", "from 0 → 1");

console.log("chapterWordStats.selfcheck ok");
