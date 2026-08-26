import {
  emptyInfoFlow,
  formatInfoFlowExtracted,
  normalizeInfoFlow,
} from "@/lib/infoFlow";

const d = normalizeInfoFlow({
  premise: "真相滞后于流言",
  info_speed: "驿马一日百里",
  info_barrier: "边境查禁",
  message_truth: "口信失真",
  knowledge_carrier: "竹简与口述",
});

const md = formatInfoFlowExtracted(d);
console.assert(md.includes("驿马"));
console.assert(md.includes("## 一句话立意"));
console.assert(md.includes("真相滞后"));
console.assert(formatInfoFlowExtracted(emptyInfoFlow()) === "");

console.log("infoFlow.selfcheck ok");
