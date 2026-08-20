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
