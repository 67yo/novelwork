/** ponytail: chapter → global chat commands. Run: npx tsx src/lib/chapterChatCommands.selfcheck.ts */
import {
  chatCmdGenerateChapter,
  chatCmdGenerateOutline,
  chatCmdRefineChapter,
} from "./chapterChatCommands.ts";

const ol = chatCmdGenerateOutline(3);
console.assert(ol.includes("生成本章细纲"), ol);
console.assert(ol.includes("第3章"), ol);
console.assert(!ol.includes("正文"), "outline must not say 正文 (write-intent)");

const gen = chatCmdGenerateChapter(3);
console.assert(gen.includes("生成第3章"), gen);
console.assert(gen.includes("正文"), gen);

const rf = chatCmdRefineChapter(12);
console.assert(rf.includes("精修第12章"), rf);
console.assert(rf.includes("正文"), rf);

console.log("chapterChatCommands.selfcheck ok");
