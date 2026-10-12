/** 最小自检：blockDataFromKnowledge 从知识卡嵌套字段读出 premise（勿把整卡当 flat）。 */
import { blockDataFromKnowledge, normalizeStoryRulesBlock, normalizeSurfaceSetting } from "./storyRules";

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

const nested = normalizeSurfaceSetting({
  premise: { text: "立意" } as unknown as string,
  core_conflict: ["冲突"] as unknown as string,
});
if (nested.premise !== "立意") throw new Error("object field should flatten to text");
if (nested.core_conflict !== "冲突") throw new Error("array field should join to text");

console.log("storyRules.selfcheck ok");
