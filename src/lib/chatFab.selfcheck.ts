/** ponytail: chat FAB right/bottom clamp. Run: npx tsx src/lib/chatFab.selfcheck.ts */
import { CHAT_FAB_SIZE, clampFabOffsets } from "./chatFab";

const def = clampFabOffsets({ x: -1, y: -1 }, 1600, 900);
if (def.right !== 16 || def.bottom !== 16) throw new Error(`default ${JSON.stringify(def)}`);

const fromXy = clampFabOffsets({ x: 1400, y: 800 }, 1600, 900);
if (fromXy.right !== 1600 - 1400 - CHAT_FAB_SIZE) throw new Error("x → right");
if (fromXy.bottom !== 900 - 800 - CHAT_FAB_SIZE) throw new Error("y → bottom");

const kept = clampFabOffsets({ right: 40, bottom: 24 }, 2000, 1200);
if (kept.right !== 40 || kept.bottom !== 24) throw new Error("keep right/bottom on resize");

const neg = clampFabOffsets({ right: -20, bottom: -4 }, 800, 600);
if (neg.right !== 8 || neg.bottom !== 8) throw new Error("clamp past edge");

const tight = clampFabOffsets({ right: 400, bottom: 400 }, 80, 80);
if (tight.right !== 24 || tight.bottom !== 24) throw new Error("tiny window");

console.log("chatFab.selfcheck ok");
