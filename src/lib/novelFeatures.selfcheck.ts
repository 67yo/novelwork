import {
  addCustomFeature,
  emptyNovelFeatures,
  normalizeNovelFeatures,
  toggleFeatureValue,
} from "./novelFeatures";

const a = emptyNovelFeatures();
const b = toggleFeatureValue(a, "genres", "urban");
console.assert(b.genres.includes("urban") && !a.genres.includes("urban"), "toggle add");
const c = toggleFeatureValue(b, "genres", "urban");
console.assert(c.genres.length === 0, "toggle remove");

const d = addCustomFeature(b, "genres", " 赛博修真 ");
console.assert(d.genres.includes("赛博修真"), "custom trim");
const e = addCustomFeature(d, "relationships", "whatever");
console.assert(e.relationships.length === 0, "no custom on relationships");

const n = normalizeNovelFeatures({
  genres: ["urban", "urban", "  ", "赛博", 1],
  audiences: ["male", "alien"],
  junk: true,
});
console.assert(
  JSON.stringify(n.genres) === JSON.stringify(["urban", "赛博"]),
  "normalize genres",
);
console.assert(JSON.stringify(n.audiences) === JSON.stringify(["male"]), "drop unknown audience");
console.assert(n.styles.length === 0, "default empty");

console.log("novelFeatures.selfcheck: ok");
