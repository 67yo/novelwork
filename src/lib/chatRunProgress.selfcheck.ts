/** node --import tsx src/lib/chatRunProgress.selfcheck.ts */
import { appendChatRunStats, createChatRunProgress } from "./chatRunProgress";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

let lines: string[] = [];
const run = createChatRunProgress({
  initialLabel: "analyzing",
  formatMs: (ms) => `${ms}ms`,
  formatUsage: (p, c, ok) => `${ok ? "" : "~"}${p}+${c}=${p + c}`,
  formatTotal: (t) => `total ${t}`,
  onLines: (l) => {
    lines = l;
  },
});

run.advance("thinking", "thinking");
run.onTokens(100, 0, false);
assert(lines.some((l) => l.includes("~100+0=100")), "est prompt");
assert(lines.some((l) => l.includes("total")), "live total");
run.onTokens(120, 40, true);
assert(lines.some((l) => l.includes("120+40=160")), "confirmed tokens");
run.onTokens(10, 5, true);
run.advance("saving", "saving");
const stats = run.finish();
assert(stats.includes("thinking"), "step in stats");
assert(stats.includes("130+45=175"), "summed tokens");
assert(appendChatRunStats("hi", stats).includes("———"), "footer sep");
console.log("chatRunProgress.selfcheck ok");
