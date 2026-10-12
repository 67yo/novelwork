/** ponytail: manuscript list drag-reorder. Run: npx tsx src/lib/chapterListReorder.selfcheck.ts */

export const FREE_CHAPTER_GROUP = "__free__";

export type ManuscriptKind = "volume" | "chapter";

export function manuscriptGroups(
  rows: { id: string; kind: ManuscriptKind; indent?: number }[],
): Map<string, string[]> {
  const groups = new Map<string, string[]>();
  let gid = FREE_CHAPTER_GROUP;
  for (const row of rows) {
    if (row.kind === "volume") {
      gid = row.id;
      if (!groups.has(gid)) groups.set(gid, []);
      continue;
    }
    if ((row.indent ?? 0) === 0) gid = FREE_CHAPTER_GROUP;
    const list = groups.get(gid) ?? [];
    list.push(row.id);
    groups.set(gid, list);
  }
  return groups;
}

export function reorderInGroup(ids: string[], fromId: string, toId: string): string[] | null {
  const from = ids.indexOf(fromId);
  const to = ids.indexOf(toId);
  if (from < 0 || to < 0 || from === to) return null;
  const next = [...ids];
  const [item] = next.splice(from, 1);
  if (!item) return null;
  next.splice(to, 0, item);
  return next;
}

export function assignChapterYs<T extends { id: string; position: { x: number; y: number } }>(
  nodes: T[],
  chapterIds: string[],
): void {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const ys = chapterIds
    .map((id) => byId.get(id)?.position.y)
    .filter((y): y is number => typeof y === "number");
  const minY = ys.length ? Math.min(...ys) : 0;
  const span = ys.length ? Math.max(...ys) - minY : 0;
  const step =
    chapterIds.length > 1 ? Math.max(8, span > 0 ? span / (chapterIds.length - 1) : 140) : 140;
  chapterIds.forEach((id, i) => {
    const n = byId.get(id);
    if (n) n.position.y = minY + i * step;
  });
}

type SpineEdge = {
  id: string;
  source: string;
  target: string;
  kind: string;
  source_handle?: string | null;
  target_handle?: string | null;
};

export function relinkChapterChain<E extends SpineEdge>(
  edges: E[],
  hostId: string,
  chapterIds: string[],
): E[] {
  const set = new Set([hostId, ...chapterIds]);
  const next = edges.filter((e) => {
    if (e.kind === "character" || e.kind === "side_plot" || e.kind === "knowledge") return true;
    return !(set.has(e.source) && set.has(e.target));
  });
  let prev = hostId;
  for (const cid of chapterIds) {
    next.push({
      id: `e-${prev}-${cid}`,
      source: prev,
      target: cid,
      kind: "chapter",
      source_handle: "bottom",
      target_handle: "top",
    } as E);
    prev = cid;
  }
  return next;
}
