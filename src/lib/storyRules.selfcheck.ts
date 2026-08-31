/** 最小自检：blockDataFromKnowledge 从知识卡嵌套字段读出 premise（勿把整卡当 flat）。 */
import { blockDataFromKnowledge, normalizeStoryRulesBlock } from "./storyRules";

const k = {
  surface_setting: {
    premise: "第三人称限知",
    core_conflict: "真相",
  },
};

const fromKn = blockDataFromKnowledge("sr_surface_setting", k);
if ((fromKn as { premise: string }).premise !== "第三人称限知") {
  throw new Error("blockDataFromKnowledge should read nested surface_setting");
}

const wrong = normalizeStoryRulesBlock("sr_surface_setting", k as Record<string, unknown>);
if ((wrong as { premise: string }).premise === "第三人称限知") {
  throw new Error("normalizeStoryRulesBlock on full knowledge must not accidentally flatten");
}

const fromLegacy = normalizeStoryRulesBlock("sr_fulfillment_system", {
  tension_circles: ["旧键"],
});
if ((fromLegacy as { tension_archetypes: string[] }).tension_archetypes[0] !== "旧键") {
  throw new Error("legacy tension_circles should map to tension_archetypes");
}

console.log("storyRules.selfcheck ok");
