import {
  emptySocialPower,
  formatSocialPowerExtracted,
  normalizeSocialPower,
  parseMajorFactionJson,
  parseWorldRaceJson,
} from "./socialPower.ts";

const d = normalizeSocialPower({
  premise: "权力在暗处流转",
  races: [
    {
      name: "角民",
      features: "头生角",
      population: "80%",
      social_status: "底层劳工",
    },
  ],
  factions: [
    {
      name: "北境同盟",
      faction_type: "政治军事联盟",
      goal: "统一关税",
      means: "联姻",
      power_base: "雾港",
    },
  ],
  class_structure: "三层金字塔",
  political_system: "寡头议会",
  power_visibility: "表面民主、暗线操控",
});
const md = formatSocialPowerExtracted(d);
console.assert(md.includes("### 角民"));
console.assert(md.includes("头生角"));
console.assert(md.includes("### 北境同盟"));
console.assert(
  formatSocialPowerExtracted(
    { ...d, races: [], factions: [] },
    [{ name: "子卡族", features: "f", population: "p", social_status: "s" }],
    [],
  ).includes("### 子卡族"),
);
console.assert(parseWorldRaceJson('{"name":"A","features":"f","population":"p","social_status":"s"}')?.name === "A");
console.assert(parseMajorFactionJson('{"faction_type":"联盟","goal":"g","means":"m","power_base":"b"}')?.faction_type === "联盟");
console.assert(formatSocialPowerExtracted(emptySocialPower()) === "");
console.log("socialPower.selfcheck ok");
