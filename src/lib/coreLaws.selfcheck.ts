import {
  emptyCoreLaws,
  formatCoreLawsExtracted,
  normalizeCoreLaws,
  parseWorldAxiomJson,
} from "./coreLaws.ts";

const d = normalizeCoreLaws({
  premise: "力量有价",
  axioms: [
    {
      name: "借力律",
      statement: "借力必还",
      boundary: "不可永久免除",
      cost: "寿命或记忆",
      mechanism: "契约自动结算",
    },
  ],
  taboos: ["严禁无代价成神，否则世界崩解"],
  power_system: "气脉九境",
  power_expression: "显化纹路",
});
const md = formatCoreLawsExtracted(d);
console.assert(md.includes("## 一句话立意"));
console.assert(md.includes("### 借力律"));
console.assert(md.includes("借力必还"));
const fromCards = formatCoreLawsExtracted(
  { ...d, axioms: [] },
  [
    {
      name: "子卡公理",
      statement: "s",
      boundary: "b",
      cost: "c",
      mechanism: "m",
    },
  ],
);
console.assert(fromCards.includes("### 子卡公理"));
console.assert(formatCoreLawsExtracted(emptyCoreLaws()) === "");
console.assert(normalizeCoreLaws(null).axioms.length === 0);

const parsed = parseWorldAxiomJson(
  '废话\n{"name":"借力律","statement":"借力必还","boundary":"b","cost":"c","mechanism":"m"}\n',
);
console.assert(parsed?.name === "借力律");
console.assert(parsed?.statement === "借力必还");
const zh = parseWorldAxiomJson('{"名称":"甲","表述":"乙","边界":"","代价":"","执行机制":""}');
console.assert(zh?.name === "甲" && zh?.statement === "乙");
console.assert(parseWorldAxiomJson("not json") === null);

console.log("coreLaws.selfcheck ok");
