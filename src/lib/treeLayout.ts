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
  if (kind === "volume") return 88;
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

type HostKind = "novel" | "volume" | "chapter" | "side_plot";

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
    if (
      h.kind !== "novel" &&
      h.kind !== "volume" &&
      h.kind !== "chapter" &&
      h.kind !== "side_plot"
    )
      return;
    const prev = host.get(cardId);
    if (!prev) {
      host.set(cardId, hostId);
      return;
    }
    const rank = (id: string) => {
      const n = byId.get(id)!;
      const k = n.kind as HostKind;
      const kr = k === "chapter" ? 0 : k === "volume" ? 1 : k === "novel" ? 2 : 3;
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
    const sHost = s.kind === "chapter" || s.kind === "novel" || s.kind === "volume";
    const tHost = t.kind === "chapter" || t.kind === "novel" || t.kind === "volume";
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

/** 剧情卡按宿主 linked_side_plot_ids 顺序（0 最上）；未入数组的排在后 */
function sortPlotsForHost(host: LayoutNode, plots: LayoutNode[]): LayoutNode[] {
  const order = host.linked_side_plot_ids ?? [];
  const idx = new Map(order.map((id, i) => [id, i]));
  return [...plots].sort((a, b) => {
    const ia = idx.has(a.id) ? idx.get(a.id)! : order.length;
    const ib = idx.has(b.id) ? idx.get(b.id)! : order.length;
    if (ia !== ib) return ia - ib;
    return byY(a, b) || a.id.localeCompare(b.id);
  });
}

/** 主轴边：排除人物/剧情/知识卡边 */
function isSpineEdge(kind: string): boolean {
  return kind !== "character" && kind !== "side_plot" && kind !== "knowledge";
}

function spineNeighbors(
  id: string,
  edges: LayoutEdge[],
  want: Set<string>,
): string[] {
  const out: string[] = [];
  for (const e of edges) {
    if (!isSpineEdge(e.kind)) continue;
    const other = e.source === id ? e.target : e.target === id ? e.source : null;
    if (other && want.has(other) && !out.includes(other)) out.push(other);
  }
  return out;
}

/** 分卷顺序：从根挂出的卷出发，沿卷↔卷链走；剩余按 y */
function orderVolumes(
  root: LayoutNode | undefined,
  volumes: LayoutNode[],
  edges: LayoutEdge[],
  byId: Map<string, LayoutNode>,
): LayoutNode[] {
  if (!volumes.length) return [];
  const volIds = new Set(volumes.map((v) => v.id));
  const ordered: LayoutNode[] = [];
  const seen = new Set<string>();

  const walk = (vid: string) => {
    if (seen.has(vid)) return;
    seen.add(vid);
    const v = byId.get(vid);
    if (v) ordered.push(v);
    const next = spineNeighbors(vid, edges, volIds)
      .filter((id) => !seen.has(id))
      .sort((a, b) => byY(byId.get(a)!, byId.get(b)!));
    for (const n of next) walk(n);
  };

  const fromRoot: string[] = [];
  if (root) {
    for (const id of spineNeighbors(root.id, edges, volIds)) fromRoot.push(id);
    fromRoot.sort((a, b) => byY(byId.get(a)!, byId.get(b)!));
  }
  for (const id of fromRoot) walk(id);
  for (const v of [...volumes].sort(byY)) walk(v.id);
  return ordered;
}

/** 一组章节：按章↔章链拓扑，否则按 y */
function orderChapterGroup(
  chapters: LayoutNode[],
  edges: LayoutEdge[],
  byId: Map<string, LayoutNode>,
): LayoutNode[] {
  if (chapters.length <= 1) return chapters;
  const ids = new Set(chapters.map((c) => c.id));
  const succ = new Map<string, string[]>();
  const predCount = new Map<string, number>();
  for (const c of chapters) predCount.set(c.id, 0);
  for (const e of edges) {
    if (!isSpineEdge(e.kind)) continue;
    if (!ids.has(e.source) || !ids.has(e.target)) continue;
    // 约定 source→target 为上游→下游；颠倒边也尽量成链
    const a = e.source;
    const b = e.target;
    const list = succ.get(a) ?? [];
    if (!list.includes(b)) {
      list.push(b);
      succ.set(a, list);
      predCount.set(b, (predCount.get(b) ?? 0) + 1);
    }
  }
  const starts = chapters
    .filter((c) => (predCount.get(c.id) ?? 0) === 0)
    .sort(byY);
  const ordered: LayoutNode[] = [];
  const seen = new Set<string>();
  const walk = (id: string) => {
    if (seen.has(id)) return;
    seen.add(id);
    const n = byId.get(id);
    if (n) ordered.push(n);
    const next = (succ.get(id) ?? [])
      .filter((x) => !seen.has(x))
      .sort((a, b) => byY(byId.get(a)!, byId.get(b)!));
    for (const x of next) walk(x);
  };
  for (const s of starts) walk(s.id);
  for (const c of [...chapters].sort(byY)) walk(c.id);
  return ordered;
}

/**
 * 主轴：根 →（分卷 → 该卷下章节）* → 无分卷章节。
 * 卷与卷按连线成链；卷下章节聚在该卷之后。
 */
function buildSpine(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): LayoutNode[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const root = nodes.find((n) => n.kind === "novel");
  const volumes = nodes.filter((n) => n.kind === "volume");
  const chapters = nodes.filter((n) => n.kind === "chapter");
  const volOrder = orderVolumes(root, volumes, edges, byId);

  const underVol = new Map<string, LayoutNode[]>();
  const freeChapters: LayoutNode[] = [];
  for (const ch of chapters) {
    const vid = chapterParentVolumeId(ch.id, nodes, edges);
    if (vid && volOrder.some((v) => v.id === vid)) {
      const list = underVol.get(vid) ?? [];
      list.push(ch);
      underVol.set(vid, list);
    } else {
      freeChapters.push(ch);
    }
  }

  const spine: LayoutNode[] = [];
  if (root) spine.push(root);
  for (const vol of volOrder) {
    spine.push(vol);
    spine.push(...orderChapterGroup(underVol.get(vol.id) ?? [], edges, byId));
  }
  spine.push(...orderChapterGroup(freeChapters, edges, byId));
  return spine;
}

/**
 * 四带：知识（更左）| 人物 | 根+分卷+章节 | 剧情。
 * 主轴按分卷结构排布（见 buildSpine）；无链接卡片时竖向贴近，有链接则按侧块高度拉开。
 * @param sizes Vue Flow `dimensions`，人物卡尤需真实高度以免重叠
 */
export function applyAutoLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  sizes?: Map<string, LayoutSize>,
): void {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const spine = buildSpine(nodes, edges);
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
    const plots = sortPlotsForHost(host, plotsByHost.get(host.id) ?? []);
    plotsByHost.set(host.id, plots);
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

/** listed 顺序优先，再并入与 host 相连、同 kind 且尚未列入的节点 */
export function unionLinkedIds(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  hostId: string,
  listed: string[],
  kind: string,
): string[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const ids = [...listed];
  for (const e of edges) {
    if (e.source !== hostId && e.target !== hostId) continue;
    const other = e.source === hostId ? e.target : e.source;
    const n = byId.get(other);
    if (n?.kind === kind && !ids.includes(other)) ids.push(other);
  }
  return ids;
}

export function rootLinkedPlotIds(nodes: LayoutNode[], edges: LayoutEdge[]): string[] {
  const root = nodes.find((n) => n.kind === "novel");
  if (!root) return [];
  return unionLinkedIds(nodes, edges, root.id, root.linked_side_plot_ids ?? [], "side_plot");
}

export function rootLinkedKnowledgeIds(nodes: LayoutNode[], edges: LayoutEdge[]): string[] {
  const root = nodes.find((n) => n.kind === "novel");
  if (!root) return [];
  return unionLinkedIds(nodes, edges, root.id, root.linked_knowledge_ids ?? [], "knowledge");
}

/** 沿 spine 找父分卷：与分卷任意相连即认；同层优先 Volume；边方向兼容 */
export function chapterParentVolumeId(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string | null {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  let cur = chapterId;
  const seen = new Set<string>();
  while (!seen.has(cur)) {
    seen.add(cur);
    let volume: string | null = null;
    let chapterParent: string | null = null;
    let hitNovel = false;
    for (const e of edges) {
      if (e.kind === "character" || e.kind === "side_plot" || e.kind === "knowledge") continue;
      const other = e.target === cur ? e.source : e.source === cur ? e.target : null;
      if (!other) continue;
      const n = byId.get(other);
      if (!n) continue;
      if (n.kind === "volume" && !volume) volume = n.id;
      else if (n.kind === "novel") hitNovel = true;
      else if (n.kind === "chapter" && !chapterParent) chapterParent = other;
    }
    if (volume) return volume;
    if (chapterParent) {
      cur = chapterParent;
      continue;
    }
    if (hitNovel) return null;
    return null;
  }
  return null;
}

function hostLinked(
  hostId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  field: "linked_side_plot_ids" | "linked_knowledge_ids" | "linked_character_ids",
  kind: string,
): string[] {
  const host = nodes.find((n) => n.id === hostId);
  return unionLinkedIds(nodes, edges, hostId, host?.[field] ?? [], kind);
}

export function chapterInheritedPlotIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const ids = [...rootLinkedPlotIds(nodes, edges)];
  const vid = chapterParentVolumeId(chapterId, nodes, edges);
  if (vid) {
    for (const pid of hostLinked(vid, nodes, edges, "linked_side_plot_ids", "side_plot")) {
      if (!ids.includes(pid)) ids.push(pid);
    }
  }
  return ids;
}

export function chapterInheritedKnowledgeIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const ids = [...rootLinkedKnowledgeIds(nodes, edges)];
  const vid = chapterParentVolumeId(chapterId, nodes, edges);
  if (vid) {
    for (const kid of hostLinked(vid, nodes, edges, "linked_knowledge_ids", "knowledge")) {
      if (!ids.includes(kid)) ids.push(kid);
    }
  }
  return ids;
}

/** 章节本机剧情（不含根/分卷继承） */
export function chapterLocalPlotIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const inherited = new Set(chapterInheritedPlotIds(chapterId, nodes, edges));
  const ch = nodes.find((n) => n.id === chapterId);
  const ids = (ch?.linked_side_plot_ids ?? []).filter((id) => !inherited.has(id));
  for (const e of edges) {
    if (e.source !== chapterId && e.target !== chapterId) continue;
    const other = e.source === chapterId ? e.target : e.source;
    if (inherited.has(other)) continue;
    const n = nodes.find((x) => x.id === other);
    if (n?.kind === "side_plot" && !ids.includes(other)) ids.push(other);
  }
  return ids;
}

/** 章节有效知识：本章 + 根/分卷继承 */
export function chapterEffectiveKnowledgeIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const ch = nodes.find((n) => n.id === chapterId);
  const ids = unionLinkedIds(
    nodes,
    edges,
    chapterId,
    ch?.linked_knowledge_ids ?? [],
    "knowledge",
  );
  for (const kid of chapterInheritedKnowledgeIds(chapterId, nodes, edges)) {
    if (!ids.includes(kid)) ids.push(kid);
  }
  return ids;
}

/** 分卷本机剧情（不含根） */
export function volumeLocalPlotIds(
  volumeId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const rootSet = new Set(rootLinkedPlotIds(nodes, edges));
  return hostLinked(volumeId, nodes, edges, "linked_side_plot_ids", "side_plot").filter(
    (id) => !rootSet.has(id),
  );
}

export function volumeEffectiveKnowledgeIds(
  volumeId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const ids = hostLinked(volumeId, nodes, edges, "linked_knowledge_ids", "knowledge");
  for (const kid of rootLinkedKnowledgeIds(nodes, edges)) {
    if (!ids.includes(kid)) ids.push(kid);
  }
  return ids;
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
