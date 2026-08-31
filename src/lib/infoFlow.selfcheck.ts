import {
  emptyInfoFlow,
  formatInfoFlowExtracted,
  normalizeInfoFlow,
} from "@/lib/infoFlow";

const d = normalizeInfoFlow({
  premise: "真相滞后于流言",
  info_speed: "驿马一日百里",
  info_barrier: "边境查禁",
  rumor_truth: "口信失真",
  knowledge_carrier: "竹简与口述",
});

const md = formatInfoFlowExtracted(d);
console.assert(md.includes("驿马"));
console.assert(md.includes("## 一句话立意"));
console.assert(md.includes("真相滞后"));
console.assert(md.includes("## 流言与真相"));
console.assert(formatInfoFlowExtracted(emptyInfoFlow()) === "");
console.assert(normalizeInfoFlow({ message_truth: "旧键" }).rumor_truth === "旧键");

console.log("infoFlow.selfcheck ok");
