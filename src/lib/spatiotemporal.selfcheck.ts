import {
  emptySpatiotemporal,
  formatSpatiotemporalExtracted,
  normalizeSpatiotemporal,
  parseKeyLocationJson,
  parseKeyLocationsJson,
} from "./spatiotemporal.ts";

const d = normalizeSpatiotemporal({
  premise: "大陆在裂变",
  era: "蒸汽与灵能交织的末法百年",
  ecology: "潮汐林与盐漠并存",
  world_pattern: "三城同盟对峙神权残部",
  locations: [
    {
      name: "盐门港",
      features: "自由贸易中立区",
      terrain: "峡湾陡崖",
      faction: "盐商公会",
    },
  ],
  atmosphere: "金属锈味与潮雾",
});
const md = formatSpatiotemporalExtracted(d);
console.assert(md.includes("## 一句话立意"));
console.assert(md.includes("### 盐门港"));
console.assert(md.includes("控制势力：盐商公会"));
const fromCards = formatSpatiotemporalExtracted(
  { ...d, locations: [] },
  [{ name: "子卡港", features: "f", terrain: "t", faction: "x" }],
);
console.assert(fromCards.includes("### 子卡港"));
console.assert(formatSpatiotemporalExtracted(emptySpatiotemporal()) === "");
const parsed = parseKeyLocationsJson(
  '说明\n[{"name":"A","features":"f","terrain":"t","faction":"x"}]\n完',
);
console.assert(parsed.length === 1 && parsed[0]!.name === "A");
const one = parseKeyLocationJson('{"name":"雾港","features":"贸易","terrain":"","faction":""}');
console.assert(one?.name === "雾港");
console.log("spatiotemporal.selfcheck ok");
