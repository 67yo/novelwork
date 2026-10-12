/** ponytail: guide markdown split. Run: npx tsx src/lib/guideMarkdown.selfcheck.ts */
import { joinGuideMarkdown, splitGuideMarkdown } from "./guideMarkdown";

const volBody = "## 本卷定位\nfoo\n\n# 三层结构\nbar";
const joined = joinGuideMarkdown([
  { title: "第一卷", body: volBody },
  { title: "第1章", body: "大纲" },
]);
const a = splitGuideMarkdown(joined, ["第一卷", "第1章"]);
console.assert(a.length === 2, a.length);
console.assert(a[0]!.title === "第一卷");
console.assert(a[0]!.body.includes("# 三层结构"));
console.assert(a[1]!.title === "第1章");
console.assert(a[1]!.body === "大纲");

const noHr = `# 第一卷\n\n${volBody}\n\n# 第1章\n大纲`;
const b = splitGuideMarkdown(noHr, ["第一卷", "第1章"]);
console.assert(b.length === 2, JSON.stringify(b));
console.assert(b[0]!.body.includes("# 三层结构"));
console.assert(b[1]!.title === "第1章");

console.log("guideMarkdown.selfcheck ok");
