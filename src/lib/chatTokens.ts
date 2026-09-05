/** Format token counts as K / M for chat usage lines. */

export type TokenUsageFields = {
  prompt_tokens?: number;
  completion_tokens?: number;
  promptTokens?: number;
  completionTokens?: number;
};

export function readMessageTokens(m: TokenUsageFields): { prompt: number; completion: number } {
  const prompt = toCount(m.prompt_tokens ?? m.promptTokens);
  const completion = toCount(m.completion_tokens ?? m.completionTokens);
  return { prompt, completion };
}

function toCount(n: unknown): number {
  const x = Math.round(Number(n));
  return Number.isFinite(x) && x > 0 ? x : 0;
}

export function formatTokenAmount(n: number): string {
  const x = Math.max(0, Math.round(Number.isFinite(n) ? n : 0));
  if (x >= 1_000_000) return `${formatShort(x / 1_000_000)}M`;
  if (x >= 1_000) return `${formatShort(x / 1_000)}K`;
  return String(x);
}

function formatShort(v: number): string {
  if (v >= 100) {
    const t = Math.round(v * 10) / 10;
    return Number.isInteger(t) ? String(t) : t.toFixed(1);
  }
  const t = Math.round(v * 100) / 100;
  return String(t);
}
