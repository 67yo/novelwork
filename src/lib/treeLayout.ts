export type LayoutNode = {
  id: string;
  kind: string;
  position: { x: number; y: number };
  linked_side_plot_ids?: string[];
};

export type LayoutEdge = {
  source: string;
  target: string;
  kind: string;
};

const CHAR_X = 40;
const MAIN_X = 320;
const PLOT_X = 640;
/** 剧情卡横排列距 */
const PLOT_DX = 220;
/** 同章剧情卡竖向紧凑间距 */
const PLOT_DY = 100;
/** 竖排最多几个，多出来的往右横排 */
const MAX_PLOT_COL = 4;
const TOP = 40;
const ROOT_TO_CH = 160;
/** 无剧情时章节之间的最小间距 */
const MIN_CHAPTER_GAP = 200;
/** 剧情块下方再留一点再放下章 */
const CH_PAD = 48;
const CHAR_DY = 150;

function byY(a: LayoutNode, b: LayoutNode) {
  return (
    a.position.y - b.position.y ||
    a.position.x - b.position.x ||
    a.id.localeCompare(b.id)
  );
}

/** 同章剧情：竖向最多 MAX_PLOT_COL，超出向右横排；上下紧凑 */
function placePlotGrid(list: LayoutNode[], originX: number, originY: number) {
  list.forEach((p, i) => {
    const col = Math.floor(i / MAX_PLOT_COL);
    const row = i % MAX_PLOT_COL;
    p.position = {
      x: originX + col * PLOT_DX,
      y: originY + row * PLOT_DY,
    };
  });
}

function plotBlockSpan(count: number): number {
  if (count <= 0) return 0;
  const rows = Math.min(count, MAX_PLOT_COL);
  return rows * PLOT_DY;
}

/** 三列：人物 | 根+章节 | 剧情。就地改 position。 */
export function applyAutoLayout(nodes: LayoutNode[], edges: LayoutEdge[]): void {
  const root = nodes.find((n) => n.kind === "novel");
  if (root) root.position = { x: MAIN_X, y: TOP };

  const chapters = nodes.filter((n) => n.kind === "chapter").sort(byY);

  nodes
    .filter((n) => n.kind === "character")
    .sort(byY)
    .forEach((n, i) => {
      n.position = { x: CHAR_X, y: TOP + i * CHAR_DY };
    });

  const charCount = nodes.filter((n) => n.kind === "character").length;
  nodes
    .filter((n) => n.kind === "knowledge")
    .sort(byY)
    .forEach((n, i) => {
      n.position = { x: CHAR_X, y: TOP + (charCount + i) * CHAR_DY };
    });

  const plotHost = new Map<string, string>();
  for (const ch of chapters) {
    for (const pid of ch.linked_side_plot_ids ?? []) plotHost.set(pid, ch.id);
  }
  for (const e of edges) {
    if (e.kind !== "side_plot") continue;
    const s = nodes.find((n) => n.id === e.source);
    const t = nodes.find((n) => n.id === e.target);
    if (!s || !t) continue;
    if (s.kind === "chapter" && t.kind === "side_plot") plotHost.set(t.id, s.id);
    else if (t.kind === "chapter" && s.kind === "side_plot") plotHost.set(s.id, t.id);
  }

  const byChapter = new Map<string, LayoutNode[]>();
  const orphan: LayoutNode[] = [];
  for (const p of nodes.filter((n) => n.kind === "side_plot")) {
    const hid = plotHost.get(p.id);
    if (hid && chapters.some((c) => c.id === hid)) {
      const list = byChapter.get(hid) ?? [];
      list.push(p);
      byChapter.set(hid, list);
    } else orphan.push(p);
  }

  // 先按剧情块高度拉开章节，再落剧情网格
  let y = TOP + ROOT_TO_CH;
  for (const ch of chapters) {
    ch.position = { x: MAIN_X, y };
    const list = (byChapter.get(ch.id) ?? []).sort(byY);
    placePlotGrid(list, PLOT_X, y);
    const span = plotBlockSpan(list.length);
    y = ch.position.y + Math.max(MIN_CHAPTER_GAP, span + CH_PAD);
  }

  if (orphan.length) {
    placePlotGrid(orphan.sort(byY), PLOT_X, y);
  }
}

export const __layoutConsts = {
  CHAR_X,
  MAIN_X,
  PLOT_X,
  PLOT_DX,
  PLOT_DY,
  MAX_PLOT_COL,
  TOP,
  MIN_CHAPTER_GAP,
  CH_PAD,
  ROOT_TO_CH,
};
