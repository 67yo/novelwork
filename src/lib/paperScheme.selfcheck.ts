/** ponytail: paper scheme parse. Run: npx tsx src/lib/paperScheme.selfcheck.ts */
import { parsePaperScheme, paperSchemeStyle, PAPER_SCHEME_IDS } from "./paperScheme";

if (parsePaperScheme(null) !== "white") throw new Error("null → white");
if (parsePaperScheme("nope") !== "white") throw new Error("bad → white");
if (parsePaperScheme("green") !== "green") throw new Error("green");
if (parsePaperScheme("night") !== "night") throw new Error("night");
for (const id of PAPER_SCHEME_IDS) {
  const style = paperSchemeStyle(id);
  if (!style["--cb-paper"] || !style["--cb-ink"]) throw new Error(`vars ${id}`);
}
console.log("paperScheme.selfcheck ok");
