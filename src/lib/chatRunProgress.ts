/** Live chat-command step timings + token totals for the pending assistant bubble. */

export type ChatRunProgress = {
  advance: (key: string, label: string) => void;
  onTokens: (prompt: number, completion: number, confirmed: boolean) => void;
  /** Close the open step, stop the ticker, return a stats block (may be empty). */
  finish: () => string;
  dispose: () => void;
};

export function createChatRunProgress(opts: {
  initialLabel: string;
  formatMs: (ms: number) => string;
  formatPrompt: (n: number, confirmed: boolean) => string;
  formatCompletion: (n: number) => string;
  formatTotal: (t: string) => string;
  onLines: (lines: string[]) => void;
}): ChatRunProgress {
  type Step = { key: string; label: string; ms: number; done: boolean };
  const steps: Step[] = [{ key: "_start", label: opts.initialLabel, ms: 0, done: false }];
  let stepStartedAt = performance.now();
  let promptSum = 0;
  let completionSum = 0;
  let promptEst: number | null = null;
  let tick: ReturnType<typeof setInterval> | null = null;
  let closed = false;

  const flush = () => {
    if (closed) return;
    const now = performance.now();
    let total = 0;
    const lines = steps.map((s) => {
      const ms = s.done ? s.ms : Math.round(now - stepStartedAt);
      total += ms;
      return `• ${s.label}  ${opts.formatMs(ms)}`;
    });
    lines.push(opts.formatTotal(opts.formatMs(total)));
    const prompt = promptSum + (promptEst ?? 0);
    if (prompt > 0 || completionSum > 0) {
      const confirmed = promptEst == null && promptSum > 0;
      let tok = opts.formatPrompt(prompt, confirmed);
      if (completionSum > 0) {
        tok += ` · ${opts.formatCompletion(completionSum)}`;
      }
      lines.push(tok);
    }
    opts.onLines(lines);
  };

  tick = setInterval(flush, 200);
  flush();

  const advance = (key: string, label: string) => {
    if (closed) return;
    const now = performance.now();
    const last = steps[steps.length - 1];
    if (last && !last.done && last.key === key) {
      flush();
      return;
    }
    if (last && !last.done) {
      last.ms = Math.round(now - stepStartedAt);
      last.done = true;
    }
    steps.push({ key, label, ms: 0, done: false });
    stepStartedAt = now;
    flush();
  };

  const onTokens = (prompt: number, completion: number, confirmed: boolean) => {
    if (closed) return;
    if (confirmed) {
      promptSum += prompt;
      completionSum += completion;
      promptEst = null;
    } else {
      promptEst = prompt;
    }
    flush();
  };

  const finish = (): string => {
    if (closed) return "";
    const now = performance.now();
    const last = steps[steps.length - 1];
    if (last && !last.done) {
      last.ms = Math.round(now - stepStartedAt);
      last.done = true;
    }
    if (tick) {
      clearInterval(tick);
      tick = null;
    }
    closed = true;
    const total = steps.reduce((s, x) => s + x.ms, 0);
    const lines = steps.map((s) => `• ${s.label}  ${opts.formatMs(s.ms)}`);
    lines.push(opts.formatTotal(opts.formatMs(total)));
    if (promptSum > 0 || completionSum > 0) {
      let tok = opts.formatPrompt(promptSum, true);
      if (completionSum > 0) {
        tok += ` · ${opts.formatCompletion(completionSum)}`;
      }
      lines.push(tok);
    }
    return lines.join("\n");
  };

  const dispose = () => {
    if (tick) {
      clearInterval(tick);
      tick = null;
    }
    closed = true;
  };

  return { advance, onTokens, finish, dispose };
}

export function appendChatRunStats(content: string, stats: string): string {
  const s = stats.trim();
  if (!s) return content;
  return `${content}\n\n———\n${s}`;
}
