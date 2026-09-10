export type DiffHunk = { type: "eq" | "del" | "add"; text: string };

export type DiffBlock =
  | { type: "eq"; lines: string[] }
  | { type: "conflict"; oldLines: string[]; newLines: string[] };

/** Collapse line hunks into unchanged runs vs old/new paragraph pairs. */
export function groupDiffHunks(hunks: DiffHunk[]): DiffBlock[] {
  const out: DiffBlock[] = [];
  for (const h of hunks) {
    const last = out[out.length - 1];
    if (h.type === "eq") {
      if (last?.type === "eq") last.lines.push(h.text);
      else out.push({ type: "eq", lines: [h.text] });
    } else if (h.type === "del") {
      if (last?.type === "conflict") last.oldLines.push(h.text);
      else out.push({ type: "conflict", oldLines: [h.text], newLines: [] });
    } else if (last?.type === "conflict") {
      last.newLines.push(h.text);
    } else {
      out.push({ type: "conflict", oldLines: [], newLines: [h.text] });
    }
  }
  return out;
}

/** Keep old or new text for one conflict; that region becomes equal on both sides. */
export function applyDiffPick(
  blocks: DiffBlock[],
  index: number,
  side: "old" | "new",
): { before: string; after: string } {
  const beforeLines: string[] = [];
  const afterLines: string[] = [];
  blocks.forEach((b, i) => {
    if (b.type === "eq") {
      beforeLines.push(...b.lines);
      afterLines.push(...b.lines);
      return;
    }
    if (i === index) {
      const chosen = side === "old" ? b.oldLines : b.newLines;
      beforeLines.push(...chosen);
      afterLines.push(...chosen);
    } else {
      beforeLines.push(...b.oldLines);
      afterLines.push(...b.newLines);
    }
  });
  return { before: beforeLines.join("\n"), after: afterLines.join("\n") };
}

/** Line-level LCS diff. Fine for chapter-sized texts (hundreds of lines). */
export function diffLines(oldText: string, newText: string): DiffHunk[] {
  const a = oldText.split("\n");
  const b = newText.split("\n");
  const n = a.length;
  const m = b.length;
  // ponytail: O(n·m) LCS; upgrade to Myers if chapters routinely exceed ~2k lines
  const dp: Uint16Array[] = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }
  const out: DiffHunk[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ type: "eq", text: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ type: "del", text: a[i++] });
    } else {
      out.push({ type: "add", text: b[j++] });
    }
  }
  while (i < n) out.push({ type: "del", text: a[i++] });
  while (j < m) out.push({ type: "add", text: b[j++] });
  return out;
}
