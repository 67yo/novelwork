import {
  emptyExistence,
  formatExistenceExtracted,
  normalizeExistence,
} from "@/lib/existence";

const d = normalizeExistence({
  premise: "生死有账",
  death: "灵魂不散",
  calendar: "双月历",
  lifespan: "凡人八十",
  disease_reproduction: "瘟疫周期十年",
});

const md = formatExistenceExtracted(d);
console.assert(md.includes("生死有账"));
console.assert(md.includes("## 死亡"));
console.assert(md.includes("双月历"));
console.assert(formatExistenceExtracted(emptyExistence()) === "");

console.log("existence.selfcheck ok");
