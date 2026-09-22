/** 与后端 count_words 一致：去掉空白后的字符数。 */
export function countWords(text: string): number {
  let n = 0;
  for (const c of text) {
    if (!/\s/u.test(c)) n += 1;
  }
  return n;
}

/** 1-based inclusive chapter range → [start, end) for Array#slice. Empty from/to = all. */
export function chapterSpan(
  len: number,
  from?: number | null,
  to?: number | null,
): [number, number] {
  if (len <= 0) return [0, 0];
  const clamp = (v: number) => Math.min(Math.max(Math.floor(v), 1), len);
  let a = from == null || !Number.isFinite(from) || from < 1 ? 1 : clamp(from);
  let b = to == null || !Number.isFinite(to) || to < 1 ? len : clamp(to);
  if (a > b) {
    const t = a;
    a = b;
    b = t;
  }
  return [a - 1, b];
}
