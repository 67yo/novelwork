/** ponytail: socket-aware bezier. Run: npx tsx src/lib/flowConnectionPath.selfcheck.ts */
import { storyConnectionPath } from "./flowConnectionPath.ts";

const v = storyConnectionPath({ x: 10, y: 0 }, { x: 10, y: 100 }, "bottom", "top");
console.assert(v.startsWith("M 10 0 C 10 "), "bottom→top stays on x");
console.assert(v.includes(", 10 ") && v.endsWith("10 100"), "arrives from above");

const h = storyConnectionPath({ x: 0, y: 20 }, { x: 80, y: 20 }, "right", "left");
console.assert(h.startsWith("M 0 20 C "), "right→left starts at y=20");
console.assert(h.includes(" 20,") || h.includes(" 20 "), "horizontal controls keep y");

console.log("flowConnectionPath ok");
