import {
  emptyReligion,
  formatHistoryCultureExtracted,
  formatWorldReligionExtracted,
  normalizeHistoryCulture,
  parseMajorEventJson,
  parseWorldReligionJson,
} from "@/lib/historyCulture";

const hc = normalizeHistoryCulture({
  premise: "礼法与商路并立",
  customs: "节庆祭河",
  economy: "盐铁专营",
  daily_slices: "早市与夜禁",
});
const rel = { name: "河神教", core_belief: "顺流则昌", followers_scope: "沿岸渔民" };
const ev = { title: "盐门之变", event: "商会夺权", long_term_impact: "中央集权加强" };
const md = formatHistoryCultureExtracted(hc, [rel], [ev]);
console.assert(md.includes("## 风俗习惯"));
console.assert(md.includes("### 河神教"));
console.assert(md.includes("### 盐门之变"));
console.assert(formatWorldReligionExtracted(emptyReligion()) === "");
console.assert(parseWorldReligionJson('{"name":"A","core_belief":"b","followers_scope":"c"}')?.name === "A");
console.assert(parseMajorEventJson('{"title":"T","event":"e","long_term_impact":"i"}')?.title === "T");
console.log("historyCulture.selfcheck ok");
