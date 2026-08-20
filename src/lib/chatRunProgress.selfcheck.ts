/** node --import tsx src/lib/chatRunProgress.selfcheck.ts */
import { appendChatRunStats, createChatRunProgress } from "./chatRunProgress";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

let lines: string[] = [];
const run = createChatRunProgress({
  initialLabel: "analyzing",
  formatMs: (ms) => `${ms}ms`,
  formatPrompt: (n, confirmed) => (confirmed ? `p=${n}` : `p~${n}`),
  formatCompletion: (n) => `c=${n}`,
  formatTotal: (t) => `total ${t}`,
  onLines: (l) => {
    lines = l;
  },
});

run.advance("thinking", "thinking");
run.onTokens(100, 0, false);
assert(lines.some((l) => l.includes("p~100")), "est prompt");
assert(lines.some((l) => l.includes("total")), "live total");
run.onTokens(120, 40, true);
assert(lines.some((l) => l.includes("p=120") && l.includes("c=40")), "confirmed tokens");
run.onTokens(10, 5, true);
run.advance("saving", "saving");
const stats = run.finish();
assert(stats.includes("thinking"), "step in stats");
assert(stats.includes("p=130") && stats.includes("c=45"), "summed tokens");
assert(appendChatRunStats("hi", stats).includes("———"), "footer sep");
console.log("chatRunProgress.selfcheck ok");
