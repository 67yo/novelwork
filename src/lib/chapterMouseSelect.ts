/** ponytail: click-count + CJK clause bounds for chapter paper. */

const CLAUSE_BREAK = /[\s。！？!?…，、；;]/;

export function clauseOffsets(line: string, offset: number): [number, number] {
  if (!line.length) return [0, 0];
  const i = Math.max(0, Math.min(offset, line.length));
  let from = i;
  let to = i;
  while (from > 0 && !CLAUSE_BREAK.test(line[from - 1]!)) from--;
  while (to < line.length && !CLAUSE_BREAK.test(line[to]!)) to++;
  if (from === to && to < line.length) to += 1;
  return [from, to];
}

let lastDown: { t: number; x: number; y: number; n: number } | null = null;

/** Ignore stale event.detail (WKWebView often reports 2/3 on a later click). */
export function reliableClickCount(ev: MouseEvent): 1 | 2 | 3 {
  const now = Date.now();
  const close =
    !!lastDown &&
    now - lastDown.t < 400 &&
    Math.abs(ev.clientX - lastDown.x) < 4 &&
    Math.abs(ev.clientY - lastDown.y) < 4;
  const n = (close ? (lastDown!.n % 3) + 1 : 1) as 1 | 2 | 3;
  lastDown = { t: now, x: ev.clientX, y: ev.clientY, n };
  return n;
}

/** test-only */
export function resetClickCount() {
  lastDown = null;
}
