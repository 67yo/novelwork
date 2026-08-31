/** 章节正文按行分段（与 renderChapterMd：单换行＝段）。 */

export function splitBodyLines(text: string): string[] {
  return text.replace(/\r\n/g, "\n").split("\n");
}

/** 用 `next`（可含多行）替换第 `index` 行，再拼回全文。 */
export function replaceBodyLine(text: string, index: number, next: string): string {
  const lines = splitBodyLines(text);
  if (index < 0 || index >= lines.length) return text;
  const parts = splitBodyLines(next.trimEnd());
  lines.splice(index, 1, ...(parts.length ? parts : [""]));
  return lines.join("\n");
}

export function nonEmptyLineIndexes(text: string): number[] {
  return splitBodyLines(text)
    .map((line, i) => (line.trim() ? i : -1))
    .filter((i) => i >= 0);
}

const SUGGEST_CTX_CAP = 800;

/** 当前段已写全文 + 上一段，给续写建议用。 */
export function bodySuggestContext(
  text: string,
  cursor: number,
): { current: string; prevParagraph: string } {
  const t = text.replace(/\r\n/g, "\n");
  const c = Math.max(0, Math.min(cursor, t.length));
  const lineStart = t.lastIndexOf("\n", Math.max(0, c - 1)) + 1;
  const lineEnd = t.indexOf("\n", c);
  const at = lineEnd === -1 ? t.length : lineEnd;
  const current = t.slice(lineStart, at).trim();
  const beforeLines = t.slice(0, lineStart).split("\n");
  while (beforeLines.length && !beforeLines[beforeLines.length - 1]!.trim()) beforeLines.pop();
  const prev = (beforeLines.pop() ?? "").trim();
  return {
    current: current.slice(-SUGGEST_CTX_CAP),
    prevParagraph: prev.slice(-SUGGEST_CTX_CAP),
  };
}

/** 把建议插到光标所在行末，另起一段。 */
export function insertBodySuggestion(
  text: string,
  cursor: number,
  suggestion: string,
): { text: string; cursor: number } {
  const t = text.replace(/\r\n/g, "\n");
  const c = Math.max(0, Math.min(cursor, t.length));
  const lineEnd = t.indexOf("\n", c);
  const at = lineEnd === -1 ? t.length : lineEnd;
  const before = t.slice(0, at);
  const after = t.slice(at);
  const seed = suggestion.trim();
  if (!seed) return { text: t, cursor: c };
  const sep = before.length === 0 || before.endsWith("\n") ? "" : "\n";
  const block = sep + seed;
  return { text: before + block + after, cursor: before.length + block.length };
}
