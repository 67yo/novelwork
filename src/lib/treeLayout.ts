export type LayoutNode = {
  id: string;
  kind: string;
  position: { x: number; y: number };
  linked_side_plot_ids?: string[];
  linked_character_ids?: string[];
  linked_knowledge_ids?: string[];
};

export type LayoutEdge = {
  source: string;
  target: string;
  kind: string;
};

/** 可选：Vue Flow 实测尺寸；缺省按 kind 估高 */
export type LayoutSize = { w: number; h: number };

const CHAR_X = 40;
const MAIN_X = 320;
const PLOT_X = 640;
/** 剧情卡横排列距 */
const PLOT_DX = 220;
/** 剧情卡视觉宽度（与 StoryNode max-width 对齐） */
const PLOT_CARD_W = 200;
/** 同章剧情卡竖向紧凑间距 */
const PLOT_DY = 100;
/** 竖排最多几个，多出来的往右横排 */
const MAX_PLOT_COL = 4;
/** 人物/知识卡：多列时向左横排 */
const SIDE_DX = 200;
/** 侧卡视觉宽度（与 StoryNode max-width 对齐） */
const SIDE_CARD_W = 200;
/** 无实测时的人物/知识竖向步长（fallback） */
const SIDE_DY = 100;
/** 侧卡之间固定空隙（叠在实测高度之外） */
const SIDE_GAP = 12;
const MAX_SIDE_COL = 4;
/** 知识最右列左缘相对人物最左列左缘：再左移 1.5 卡宽（卡宽 + 半卡空隙） */
const KNOW_GAP_FROM_CHAR = SIDE_CARD_W + SIDE_CARD_W / 2;
const TOP = 40;
/** 无链接时主轴节点（根/章）之间的紧凑间距 */
const MIN_CHAPTER_GAP_TIGHT = 72;
/** 有链接时章节之间的最小间距 */
const MIN_CHAPTER_GAP = 200;
/** 剧情块下方再留一点再放下章 */
const CH_PAD = 48;

function defaultNodeH(kind: string): number {
  if (kind === "character") return 96;
  if (kind === "knowledge") return 88;
  if (kind === "side_plot") return 72;
  if (kind === "novel") return 100;
  return 64;
}

function nodeH(n: LayoutNode, sizes?: Map<string, LayoutSize>): number {
  const h = sizes?.get(n.id)?.h;
  if (typeof h === "number" && h > 8) return h;
  return defaultNodeH(n.kind);
}

function byY(a: LayoutNode, b: LayoutNode) {
  return (
    a.position.y - b.position.y ||
    a.position.x - b.position.x ||
    a.id.localeCompare(b.id)
  );
}

/** 竖向最多 MAX 个，超出横排；dir=+1 向右，-1 向左 */
function placeGrid(
  list: LayoutNode[],
  originX: number,
  originY: number,
  dx: number,
  dy: number,
  maxCol: number,
  dir: 1 | -1,
) {
  list.forEach((p, i) => {
    const col = Math.floor(i / maxCol);
    const row = i % maxCol;
    p.position = {
      x: originX + dir * col * dx,
      y: originY + row * dy,
    };
  });
}

function gridSpan(count: number, dy: number, maxCol: number): number {
  if (count <= 0) return 0;
  const rows = Math.min(count, maxCol);
  return rows * dy;
}

/** 侧卡块高度（不改 position）：按实测/估高叠满 maxCol 列 */
function sideStackSpan(
  list: LayoutNode[],
  maxCol: number,
  sizes?: Map<string, LayoutSize>,
): number {
  if (!list.length) return 0;
  const nextY: number[] = [];
  const counts: number[] = [];
  let maxBottom = 0;
  for (const n of list) {
    let col = 0;
    while ((counts[col] ?? 0) >= maxCol) col++;
    const y = nextY[col] ?? 0;
    const h = nodeH(n, sizes);
    const bottom = y + h;
    nextY[col] = bottom + SIDE_GAP;
    counts[col] = (counts[col] ?? 0) + 1;
    maxBottom = Math.max(maxBottom, bottom);
  }
  return maxBottom;
}

/**
 * 人物/知识：按实测高度竖向叠放，满 MAX_SIDE_COL 张换下一列（向 dir）。
 * @returns 相对 originY 的块高度
 */
function placeSideByHeight(
  list: LayoutNode[],
  originX: number,
  originY: number,
  dx: number,
  maxCol: number,
  dir: 1 | -1,
  sizes?: Map<string, LayoutSize>,
): number {
  if (!list.length) return 0;
  const nextY: number[] = [];
  const counts: number[] = [];
  let maxBottom = originY;
  for (const n of list) {
    let col = 0;
    while ((counts[col] ?? 0) >= maxCol) col++;
    const y = nextY[col] ?? originY;
    n.position = { x: originX + dir * col * dx, y };
    const h = nodeH(n, sizes);
    const bottom = y + h;
    nextY[col] = bottom + SIDE_GAP;
    counts[col] = (counts[col] ?? 0) + 1;
    maxBottom = Math.max(maxBottom, bottom);
  }
  return Math.max(0, maxBottom - originY);
}

/**
 * 知识卡排在人物左侧：最右知识卡右缘与最左人物卡左缘间距 = 半个卡宽。
 * 无人物时相对 CHAR_X 定位。
 */
function placeKnowLeftOfChars(
  knows: LayoutNode[],
  chars: LayoutNode[],
  originY: number,
  sizes?: Map<string, LayoutSize>,
): number {
  if (!knows.length) return 0;
  const leftCharX = chars.length
    ? Math.min(...chars.map((c) => c.position.x))
    : CHAR_X;
  const originX = leftCharX - KNOW_GAP_FROM_CHAR;
  return placeSideByHeight(
    knows,
    originX,
    originY,
    SIDE_DX,
    MAX_SIDE_COL,
    -1,
    sizes,
  );
}

/** 与已占剧情格是否重叠（按排版间距估） */
function plotOverlaps(ax: number, ay: number, bx: number, by: number): boolean {
  return Math.abs(ax - bx) < PLOT_DX * 0.9 && Math.abs(ay - by) < PLOT_DY * 0.9;
}

type HostKind = "novel" | "chapter" | "side_plot";

function resolveHosts(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  cardKind: "character" | "knowledge",
  linkField: "linked_character_ids" | "linked_knowledge_ids",
  edgeKind: string,
): Map<string, string> {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const host = new Map<string, string>();

  const consider = (cardId: string, hostId: string) => {
    const h = byId.get(hostId);
    const c = byId.get(cardId);
    if (!h || !c || c.kind !== cardKind) return;
    if (h.kind !== "novel" && h.kind !== "chapter" && h.kind !== "side_plot") return;
    const prev = host.get(cardId);
    if (!prev) {
      host.set(cardId, hostId);
      return;
    }
    const rank = (id: string) => {
      const n = byId.get(id)!;
      const k = n.kind as HostKind;
      const kr = k === "chapter" ? 0 : k === "novel" ? 1 : 2;
      return [kr, n.position.y, n.position.x, id] as const;
    };
    const a = rank(prev);
    const b = rank(hostId);
    for (let i = 0; i < a.length; i++) {
      if (a[i]! < b[i]!) return;
      if (a[i]! > b[i]!) {
        host.set(cardId, hostId);
        return;
      }
    }
  };

  for (const n of nodes) {
    for (const id of n[linkField] ?? []) consider(id, n.id);
  }
  for (const e of edges) {
    if (e.kind !== edgeKind) continue;
    const s = byId.get(e.source);
    const t = byId.get(e.target);
    if (!s || !t) continue;
    if (s.kind === cardKind) consider(s.id, t.id);
    if (t.kind === cardKind) consider(t.id, s.id);
  }
  return host;
}

/** 侧卡挂在剧情上时，尽量归到该剧情所属章节，方便跟章节对齐 */
function liftSidePlotHosts(
  host: Map<string, string>,
  plotPrimaryHost: Map<string, string>,
  byId: Map<string, LayoutNode>,
) {
  for (const [cardId, hid] of [...host.entries()]) {
    const h = byId.get(hid);
    if (h?.kind === "side_plot") {
      const ch = plotPrimaryHost.get(hid);
      if (ch) host.set(cardId, ch);
    }
  }
}

/** 剧情 ↔ 主轴宿主（根/章节）：收集全部关联（边 + linked_side_plot_ids） */
function collectPlotHosts(
  hosts: LayoutNode[],
  edges: LayoutEdge[],
  byId: Map<string, LayoutNode>,
): Map<string, string[]> {
  const hostIds = new Set(hosts.map((c) => c.id));
  const map = new Map<string, Set<string>>();
  const add = (plotId: string, hostId: string) => {
    if (!hostIds.has(hostId)) return;
    const p = byId.get(plotId);
    if (!p || p.kind !== "side_plot") return;
    let set = map.get(plotId);
    if (!set) {
      set = new Set();
      map.set(plotId, set);
    }
    set.add(hostId);
  };

  for (const h of hosts) {
    for (const pid of h.linked_side_plot_ids ?? []) add(pid, h.id);
  }
  for (const e of edges) {
    if (e.kind !== "side_plot") continue;
    const s = byId.get(e.source);
    const t = byId.get(e.target);
    if (!s || !t) continue;
    const sHost = s.kind === "chapter" || s.kind === "novel";
    const tHost = t.kind === "chapter" || t.kind === "novel";
    if (sHost && t.kind === "side_plot") add(t.id, s.id);
    else if (tHost && s.kind === "side_plot") add(s.id, t.id);
  }

  const out = new Map<string, string[]>();
  for (const [pid, set] of map) {
    const list = [...set].sort((a, b) => {
      const ca = byId.get(a)!;
      const cb = byId.get(b)!;
      return byY(ca, cb);
    });
    out.set(pid, list);
  }
  return out;
}

/** 主轴节点是否链了其他卡片（有则拉开，无则贴近） */
function hostHasLinkedCards(
  host: LayoutNode,
  plotHosts: Map<string, string[]>,
  plotsByHost: Map<string, LayoutNode[]>,
  charsByHost: Map<string, LayoutNode[]>,
  knowsByHost: Map<string, LayoutNode[]>,
): boolean {
  if ((plotsByHost.get(host.id)?.length ?? 0) > 0) return true;
  if ((charsByHost.get(host.id)?.length ?? 0) > 0) return true;
  if ((knowsByHost.get(host.id)?.length ?? 0) > 0) return true;
  for (const chs of plotHosts.values()) {
    if (chs.includes(host.id)) return true;
  }
  if ((host.linked_side_plot_ids?.length ?? 0) > 0) return true;
  if ((host.linked_character_ids?.length ?? 0) > 0) return true;
  if ((host.linked_knowledge_ids?.length ?? 0) > 0) return true;
  return false;
}

/**
 * 多章共用剧情：落在所链章节垂直中点。
 * 最左一列相对所链章「单链剧情最右」再空半个卡宽（卡右缘 → 多链卡左缘）。
 */
function placeMultiChapterPlots(
  multis: LayoutNode[],
  plotChapters: Map<string, string[]>,
  byId: Map<string, LayoutNode>,
  plotsByChapter: Map<string, LayoutNode[]>,
  occupied: { x: number; y: number }[],
) {
  const ranked = [...multis].sort((a, b) => {
    const mid = (p: LayoutNode) => {
      const chs = (plotChapters.get(p.id) ?? [])
        .map((id) => byId.get(id))
        .filter((n): n is LayoutNode => !!n);
      if (!chs.length) return p.position.y;
      const ys = chs.map((c) => c.position.y);
      return (Math.min(...ys) + Math.max(...ys)) / 2;
    };
    return mid(a) - mid(b) || a.id.localeCompare(b.id);
  });

  for (const p of ranked) {
    const chIds = plotChapters.get(p.id) ?? [];
    const chs = chIds
      .map((id) => byId.get(id))
      .filter((n): n is LayoutNode => !!n);
    const ys = chs.map((c) => c.position.y);
    const midY =
      ys.length > 0 ? (Math.min(...ys) + Math.max(...ys)) / 2 : p.position.y;

    let rightmostSingleX = -Infinity;
    for (const cid of chIds) {
      for (const ex of plotsByChapter.get(cid) ?? []) {
        rightmostSingleX = Math.max(rightmostSingleX, ex.position.x);
      }
    }
    // 单链卡左缘 + 卡宽 = 右缘；再 + 半卡宽 = 多链卡左缘
    const baseX =
      rightmostSingleX === -Infinity
        ? PLOT_X
        : rightmostSingleX + PLOT_CARD_W + PLOT_CARD_W / 2;

    let x = baseX;
    // ponytail: 若与已占（含其他多链）重叠则再右移；天花板防死循环
    for (let step = 0; step < 20; step++) {
      const cx = baseX + step * PLOT_DX;
      if (!occupied.some((o) => plotOverlaps(cx, midY, o.x, o.y))) {
        x = cx;
        break;
      }
    }
    p.position = { x, y: midY };
    occupied.push({ x, y: midY });
  }
}

/**
 * 四带：知识（更左）| 人物 | 根+章节 | 剧情。
 * 主轴（根与章节）按 y 排序一列排布；无链接卡片时竖向贴近，有链接则按侧块高度拉开。
 * @param sizes Vue Flow `dimensions`，人物卡尤需真实高度以免重叠
 */
export function applyAutoLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  sizes?: Map<string, LayoutSize>,
): void {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const root = nodes.find((n) => n.kind === "novel");
  const chapters = nodes.filter((n) => n.kind === "chapter");
  // 根与章节同一主轴，按当前 y 排序（根也当章节卡处理）
  const spine = [...(root ? [root] : []), ...chapters].sort(byY);
  const plotHosts = collectPlotHosts(spine, edges, byId);

  // 人物宿主解析：多宿主剧情取最早一宿主作 primary
  const plotPrimaryHost = new Map<string, string>();
  for (const [pid, chs] of plotHosts) {
    if (chs[0]) plotPrimaryHost.set(pid, chs[0]);
  }

  const charHost = resolveHosts(nodes, edges, "character", "linked_character_ids", "character");
  const knowHost = resolveHosts(nodes, edges, "knowledge", "linked_knowledge_ids", "knowledge");
  liftSidePlotHosts(charHost, plotPrimaryHost, byId);
  liftSidePlotHosts(knowHost, plotPrimaryHost, byId);

  const plotsByHost = new Map<string, LayoutNode[]>();
  const multiPlots: LayoutNode[] = [];
  const orphanPlots: LayoutNode[] = [];
  for (const p of nodes.filter((n) => n.kind === "side_plot")) {
    const chs = plotHosts.get(p.id) ?? [];
    if (chs.length >= 2) {
      multiPlots.push(p);
    } else if (chs.length === 1) {
      const hid = chs[0]!;
      const list = plotsByHost.get(hid) ?? [];
      list.push(p);
      plotsByHost.set(hid, list);
    } else {
      orphanPlots.push(p);
    }
  }

  const charsByHost = new Map<string, LayoutNode[]>();
  const knowsByHost = new Map<string, LayoutNode[]>();
  const orphanChars: LayoutNode[] = [];
  const orphanKnows: LayoutNode[] = [];
  const spineIds = new Set(spine.map((h) => h.id));
  const pushHosted = (
    n: LayoutNode,
    hid: string | undefined,
    byHost: Map<string, LayoutNode[]>,
    orphans: LayoutNode[],
  ) => {
    if (hid && spineIds.has(hid)) {
      const list = byHost.get(hid) ?? [];
      list.push(n);
      byHost.set(hid, list);
    } else orphans.push(n);
  };
  for (const n of nodes.filter((n) => n.kind === "character")) {
    pushHosted(n, charHost.get(n.id), charsByHost, orphanChars);
  }
  for (const n of nodes.filter((n) => n.kind === "knowledge")) {
    pushHosted(n, knowHost.get(n.id), knowsByHost, orphanKnows);
  }
  for (const list of charsByHost.values()) list.sort(byY);
  for (const list of knowsByHost.values()) list.sort(byY);

  const occupiedPlots: { x: number; y: number }[] = [];

  let y = TOP;
  for (const host of spine) {
    host.position = { x: MAIN_X, y };
    const plots = (plotsByHost.get(host.id) ?? []).sort(byY);
    const chars = charsByHost.get(host.id) ?? [];
    const knows = knowsByHost.get(host.id) ?? [];
    placeGrid(plots, PLOT_X, y, PLOT_DX, PLOT_DY, MAX_PLOT_COL, 1);
    for (const p of plots) occupiedPlots.push({ ...p.position });
    const charSpan = placeSideByHeight(
      chars,
      CHAR_X,
      y,
      SIDE_DX,
      MAX_SIDE_COL,
      -1,
      sizes,
    );
    const knowSpan = placeKnowLeftOfChars(knows, chars, y, sizes);
    const span = Math.max(
      gridSpan(plots.length, PLOT_DY, MAX_PLOT_COL),
      charSpan,
      knowSpan,
    );
    const linked = hostHasLinkedCards(host, plotHosts, plotsByHost, charsByHost, knowsByHost);
    const step = linked
      ? Math.max(MIN_CHAPTER_GAP, span + CH_PAD)
      : Math.max(MIN_CHAPTER_GAP_TIGHT, nodeH(host, sizes) + SIDE_GAP);
    y = host.position.y + step;
  }

  // 多宿主共用：垂直中点；最左列距单链最右半个卡宽
  placeMultiChapterPlots(multiPlots, plotHosts, byId, plotsByHost, occupiedPlots);

  if (orphanPlots.length) {
    placeGrid(orphanPlots.sort(byY), PLOT_X, y, PLOT_DX, PLOT_DY, MAX_PLOT_COL, 1);
    for (const p of orphanPlots) occupiedPlots.push({ ...p.position });
    y += gridSpan(orphanPlots.length, PLOT_DY, MAX_PLOT_COL) + CH_PAD;
  }
  if (orphanChars.length) {
    placeSideByHeight(orphanChars.sort(byY), CHAR_X, y, SIDE_DX, MAX_SIDE_COL, -1, sizes);
  }
  if (orphanKnows.length) {
    placeKnowLeftOfChars(orphanKnows.sort(byY), orphanChars, y, sizes);
  }
}

export const __layoutConsts = {
  CHAR_X,
  KNOW_GAP_FROM_CHAR,
  SIDE_CARD_W,
  MAIN_X,
  PLOT_X,
  PLOT_DX,
  PLOT_CARD_W,
  PLOT_DY,
  MAX_PLOT_COL,
  SIDE_DX,
  SIDE_DY,
  SIDE_GAP,
  MAX_SIDE_COL,
  TOP,
  MIN_CHAPTER_GAP,
  MIN_CHAPTER_GAP_TIGHT,
  CH_PAD,
};
