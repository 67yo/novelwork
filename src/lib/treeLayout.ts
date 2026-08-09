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

const CHAR_X = 40;
const MAIN_X = 320;
const PLOT_X = 640;
/** 剧情卡横排列距 */
const PLOT_DX = 220;
/** 同章剧情卡竖向紧凑间距 */
const PLOT_DY = 100;
/** 竖排最多几个，多出来的往右横排 */
const MAX_PLOT_COL = 4;
/** 人物/知识卡：多列时向左横排 */
const SIDE_DX = 200;
const SIDE_DY = 100;
const MAX_SIDE_COL = 4;
const TOP = 40;
const ROOT_TO_CH = 160;
/** 无剧情时章节之间的最小间距 */
const MIN_CHAPTER_GAP = 200;
/** 剧情块下方再留一点再放下章 */
const CH_PAD = 48;

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
  plotHost: Map<string, string>,
  byId: Map<string, LayoutNode>,
) {
  for (const [cardId, hid] of [...host.entries()]) {
    const h = byId.get(hid);
    if (h?.kind === "side_plot") {
      const ch = plotHost.get(hid);
      if (ch) host.set(cardId, ch);
    }
  }
}

/**
 * 三列：人物/知识 | 根+章节 | 剧情。
 * 人物/知识按宿主（章/根）左侧紧凑网格；剧情按章右侧网格。就地改 position。
 */
export function applyAutoLayout(nodes: LayoutNode[], edges: LayoutEdge[]): void {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const root = nodes.find((n) => n.kind === "novel");
  if (root) root.position = { x: MAIN_X, y: TOP };

  const chapters = nodes.filter((n) => n.kind === "chapter").sort(byY);

  const plotHost = new Map<string, string>();
  for (const ch of chapters) {
    for (const pid of ch.linked_side_plot_ids ?? []) plotHost.set(pid, ch.id);
  }
  for (const e of edges) {
    if (e.kind !== "side_plot") continue;
    const s = byId.get(e.source);
    const t = byId.get(e.target);
    if (!s || !t) continue;
    if (s.kind === "chapter" && t.kind === "side_plot") plotHost.set(t.id, s.id);
    else if (t.kind === "chapter" && s.kind === "side_plot") plotHost.set(s.id, t.id);
  }

  const charHost = resolveHosts(nodes, edges, "character", "linked_character_ids", "character");
  const knowHost = resolveHosts(nodes, edges, "knowledge", "linked_knowledge_ids", "knowledge");
  liftSidePlotHosts(charHost, plotHost, byId);
  liftSidePlotHosts(knowHost, plotHost, byId);

  const plotsByChapter = new Map<string, LayoutNode[]>();
  const orphanPlots: LayoutNode[] = [];
  for (const p of nodes.filter((n) => n.kind === "side_plot")) {
    const hid = plotHost.get(p.id);
    if (hid && chapters.some((c) => c.id === hid)) {
      const list = plotsByChapter.get(hid) ?? [];
      list.push(p);
      plotsByChapter.set(hid, list);
    } else orphanPlots.push(p);
  }

  const sideByHost = new Map<string, LayoutNode[]>();
  const orphanSide: LayoutNode[] = [];
  const pushSide = (n: LayoutNode, hid: string | undefined) => {
    if (hid && (hid === root?.id || chapters.some((c) => c.id === hid))) {
      const list = sideByHost.get(hid) ?? [];
      list.push(n);
      sideByHost.set(hid, list);
    } else orphanSide.push(n);
  };
  for (const n of nodes.filter((n) => n.kind === "character")) {
    pushSide(n, charHost.get(n.id));
  }
  for (const n of nodes.filter((n) => n.kind === "knowledge")) {
    pushSide(n, knowHost.get(n.id));
  }
  for (const list of sideByHost.values()) {
    list.sort((a, b) => {
      // 同宿主：人物在前，知识在后，再按原 y
      const ka = a.kind === "character" ? 0 : 1;
      const kb = b.kind === "character" ? 0 : 1;
      return ka - kb || byY(a, b);
    });
  }

  const rootSide = root ? (sideByHost.get(root.id) ?? []).slice() : [];
  const rootSideSpan = gridSpan(rootSide.length, SIDE_DY, MAX_SIDE_COL);

  // 先按左右块高度拉开章节，再落网格
  let y = TOP + Math.max(ROOT_TO_CH, rootSideSpan + CH_PAD);
  for (const ch of chapters) {
    ch.position = { x: MAIN_X, y };
    const plots = (plotsByChapter.get(ch.id) ?? []).sort(byY);
    const side = sideByHost.get(ch.id) ?? [];
    const span = Math.max(
      gridSpan(plots.length, PLOT_DY, MAX_PLOT_COL),
      gridSpan(side.length, SIDE_DY, MAX_SIDE_COL),
    );
    placeGrid(plots, PLOT_X, y, PLOT_DX, PLOT_DY, MAX_PLOT_COL, 1);
    placeGrid(side, CHAR_X, y, SIDE_DX, SIDE_DY, MAX_SIDE_COL, -1);
    y = ch.position.y + Math.max(MIN_CHAPTER_GAP, span + CH_PAD);
  }

  if (root && rootSide.length) {
    placeGrid(rootSide, CHAR_X, TOP, SIDE_DX, SIDE_DY, MAX_SIDE_COL, -1);
  }

  if (orphanPlots.length) {
    placeGrid(orphanPlots.sort(byY), PLOT_X, y, PLOT_DX, PLOT_DY, MAX_PLOT_COL, 1);
    y += gridSpan(orphanPlots.length, PLOT_DY, MAX_PLOT_COL) + CH_PAD;
  }
  if (orphanSide.length) {
    orphanSide.sort((a, b) => {
      const ka = a.kind === "character" ? 0 : 1;
      const kb = b.kind === "character" ? 0 : 1;
      return ka - kb || byY(a, b);
    });
    placeGrid(orphanSide, CHAR_X, y, SIDE_DX, SIDE_DY, MAX_SIDE_COL, -1);
  }
}

export const __layoutConsts = {
  CHAR_X,
  MAIN_X,
  PLOT_X,
  PLOT_DX,
  PLOT_DY,
  MAX_PLOT_COL,
  SIDE_DX,
  SIDE_DY,
  MAX_SIDE_COL,
  TOP,
  MIN_CHAPTER_GAP,
  CH_PAD,
  ROOT_TO_CH,
};
