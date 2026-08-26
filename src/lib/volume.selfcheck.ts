import {
  emptyVolume,
  formatVolumeExtracted,
  normalizeVolume,
  parseVolumeFromFormatted,
  volumeDisplaySummary,
} from "./volume";

const d = normalizeVolume({
  positioning: "本卷讲逃亡",
  layer_setup: { chapters: "1-5", description: "上路" },
  conflict_external: "追兵",
  key_beats: [{ order: 2, cost: "暴露身份", description: "救人" }],
});
if (d.positioning !== "本卷讲逃亡") throw new Error("positioning");
if (d.layer_confrontation.chapters !== "") throw new Error("empty layer");
if (d.key_beats[0].order !== 2) throw new Error("beat order");
const text = formatVolumeExtracted(d);
if (!text.includes("本卷定位") || !text.includes("1-5") || !text.includes("追兵")) {
  throw new Error("format missing sections");
}
const back = parseVolumeFromFormatted(text);
if (back.positioning !== "本卷讲逃亡") throw new Error("parse positioning");
if (volumeDisplaySummary({ ...emptyVolume(), positioning: "摘要测试" }) !== "摘要测试")
  throw new Error("display summary");
console.log("volume.selfcheck ok");
