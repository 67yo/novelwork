/** ponytail: paper scheme parse. Run: npx tsx src/lib/paperScheme.selfcheck.ts */
import { parsePaperScheme, paperSchemeStyle, paperSchemeVars, PAPER_SCHEME_IDS } from "./paperScheme";

if (parsePaperScheme(null) !== "white") throw new Error("null → white");
if (parsePaperScheme("nope") !== "white") throw new Error("bad → white");
if (parsePaperScheme("green") !== "green") throw new Error("green");
if (parsePaperScheme("night") !== "night") throw new Error("night");
for (const id of PAPER_SCHEME_IDS) {
  const style = paperSchemeStyle(id);
  if (!style["--cb-paper"] || !style["--cb-ink"]) throw new Error(`vars ${id}`);
}

function hexLum(hex: string): number {
  const n = hex.replace("#", "");
  const rgb = [0, 2, 4].map((i) => {
    const c = parseInt(n.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
}
function contrast(a: string, b: string): number {
  const L1 = hexLum(a);
  const L2 = hexLum(b);
  const hi = Math.max(L1, L2);
  const lo = Math.min(L1, L2);
  return (hi + 0.05) / (lo + 0.05);
}
const green = paperSchemeVars("green");
if (contrast(green.ink, green.paper) < 14.5) throw new Error("green ink too light");
if (contrast(green.muted, green.paper) < 4.5) throw new Error("green muted too light");
console.log("paperScheme.selfcheck ok");
