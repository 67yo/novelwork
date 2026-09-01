export type LayoutNode = {
  id: string;
  kind: string;
  position: { x: number; y: number };
  linked_side_plot_ids?: string[];
  linked_character_ids?: string[];
  linked_knowledge_ids?: string[];
  knowledge?: { slot?: string } | null;
};

export type LayoutEdge = {
  source: string;
  target: string;
  kind: string;
  source_handle?: string | null;
  target_handle?: string | null;
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
/** 根上方世界观扇形半径（卡中心相对根顶） */
const WV_FAN_R = 280;
/** 故事规则右侧四卡扇形 */
const STORY_RULES_FAN_R = 200;
const STORY_RULES_FAN_SPAN_DEG = 72;
/** 核心法则上方公理扇形半径 */
const AXIOM_FAN_R = 200;
/** 时空地理上方关键地点扇形半径 */
const LOCATION_FAN_R = 200;
/** 社会权力上方种族/势力扇形半径 */
const SOCIAL_EXT_FAN_R = 200;
/** 扇形总张角（度，相对竖直向上） */
const WV_FAN_SPAN_DEG = 110;
/** 扩展子卡扇形张角（更窄，减少与相邻主卡重叠） */
const EXT_FAN_SPAN_DEG = 56;
const MIN_EXT_FAN_SPAN_DEG = 28;
const WV_FIX_MAX_PASSES = 12;
/** 扇形相邻卡最小弧长间隙 */
const WV_CARD_GAP = 16;
const MAX_FAN_R = 520;
const MAX_FAN_SPAN_DEG = 168;
/** 种族/势力双半扇中间留白（度） */
const SOCIAL_HALF_GAP_DEG = 24;
/** 根卡视觉宽度估算（用于扇形圆心） */
const ROOT_CARD_W = 200;

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

function knowledgeSlotOf(n: LayoutNode): string {
  return (n.knowledge?.slot ?? "").trim();
}

function writePromptsHubOf(nodes: LayoutNode[]): LayoutNode | undefined {
  return nodes.find((n) => n.kind === "knowledge" && knowledgeSlotOf(n) === "write_prompts");
}

function writePromptHubSide(
  hubId: string,
  edge: LayoutEdge,
  nodes: LayoutNode[],
): "generate" | "refine" | null {
  const handle =
    edge.source === hubId
      ? edge.source_handle
      : edge.target === hubId
        ? edge.target_handle
        : null;
  const h = (handle ?? "").trim();
  if (h === "right") return "refine";
  if (h === "left") return "generate";
  const other = edge.source === hubId ? edge.target : edge.target === hubId ? edge.source : "";
  if (!other) return null;
  const hub = nodes.find((n) => n.id === hubId);
  const o = nodes.find((n) => n.id === other);
  if (!hub || !o) return null;
  return o.position.x < hub.position.x ? "generate" : "refine";
}

function writePromptKids(
  hub: LayoutNode,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  kind: "generate" | "refine",
): LayoutNode[] {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const out: LayoutNode[] = [];
  for (const e of edges) {
    if (e.kind !== "knowledge") continue;
    const other = e.source === hub.id ? e.target : e.target === hub.id ? e.source : "";
    if (!other || other === hub.id) continue;
    const n = byId.get(other);
    if (!n || n.kind !== "knowledge") continue;
    if (knowledgeSlotOf(n) === "write_prompts") continue;
    if (writePromptHubSide(hub.id, e, nodes) !== kind) continue;
    if (!out.some((x) => x.id === n.id)) out.push(n);
  }
  return out;
}

const WRITE_PROMPTS_HUB_X = CHAR_X - KNOW_GAP_FROM_CHAR - SIDE_DX;

const WV_FAN_ORDER = [
  "wv_core_laws",
  "wv_spatiotemporal",
  "wv_social_power",
  "wv_history_culture",
  "wv_existence",
  "wv_info_flow",
];

/** 按卡宽与张角估算半径/张角，避免扇形内卡压卡 */
function resolveFanGeometry(
  count: number,
  baseRadius: number,
  baseSpanDeg: number,
  cardW = SIDE_CARD_W,
  gap = WV_CARD_GAP,
): { radius: number; spanDeg: number } {
  if (count <= 1) return { radius: baseRadius, spanDeg: 0 };
  const minArc = cardW + gap;
  let spanDeg = baseSpanDeg;
  let radius = Math.max(
    baseRadius,
    ((count - 1) * minArc * 180) / (Math.PI * spanDeg),
  );
  if (radius > MAX_FAN_R) {
    radius = MAX_FAN_R;
    spanDeg = ((count - 1) * minArc * 180) / (Math.PI * radius);
  }
  spanDeg = Math.min(MAX_FAN_SPAN_DEG, Math.max(baseSpanDeg, spanDeg));
  radius = Math.max(
    radius,
    ((count - 1) * minArc * 180) / (Math.PI * spanDeg),
  );
  return { radius, spanDeg };
}

type FanBBox = { id: string; x: number; y: number; w: number; h: number };

function fanBBox(n: LayoutNode, sizes?: Map<string, LayoutSize>): FanBBox {
  const w = sizes?.get(n.id)?.w ?? SIDE_CARD_W;
  const h = nodeH(n, sizes);
  return { id: n.id, x: n.position.x, y: n.position.y, w, h };
}

function fanRectsOverlap(a: FanBBox, b: FanBBox, pad = WV_CARD_GAP / 2): boolean {
  return !(
    a.x + a.w + pad <= b.x ||
    b.x + b.w + pad <= a.x ||
    a.y + a.h + pad <= b.y ||
    b.y + b.h + pad <= a.y
  );
}

const WORLDVIEW_LAYOUT_SLOTS = new Set([
  ...WV_FAN_ORDER,
  "wv_axiom",
  "wv_location",
  "wv_race",
  "wv_faction",
  "wv_religion",
  "wv_major_event",
]);

function isWorldviewLayoutNode(n: LayoutNode): boolean {
  return n.kind === "knowledge" && WORLDVIEW_LAYOUT_SLOTS.has(knowledgeSlotOf(n));
}

/** 六张世界观卡扇形排在根上方；圆心取根顶中心。也可用于公理挂核心法则。 */
function placeHalfFan(
  cards: LayoutNode[],
  anchor: LayoutNode,
  baseRadius: number,
  degMin: number,
  degMax: number,
  sizes?: Map<string, LayoutSize>,
) {
  const n = cards.length;
  if (!n) return;
  const span = Math.max(degMax - degMin, 1);
  const { radius } = resolveFanGeometry(n, baseRadius, span);
  const cx = anchor.position.x + ROOT_CARD_W / 2;
  const cy = anchor.position.y;
  cards.forEach((card, i) => {
    const deg = n === 1 ? (degMin + degMax) / 2 : degMin + ((degMax - degMin) * i) / (n - 1);
    const rad = (deg * Math.PI) / 180;
    const h = nodeH(card, sizes);
    card.position = {
      x: cx + radius * Math.sin(rad) - SIDE_CARD_W / 2,
      y: cy - radius * Math.cos(rad) - h,
    };
  });
}

function placeFanAbove(
  cards: LayoutNode[],
  anchor: LayoutNode,
  baseRadius: number,
  sizes?: Map<string, LayoutSize>,
  spanDeg = WV_FAN_SPAN_DEG,
) {
  const n = cards.length;
  if (!n) return;
  const geo = resolveFanGeometry(n, baseRadius, spanDeg);
  const cx = anchor.position.x + ROOT_CARD_W / 2;
  const cy = anchor.position.y;
  const start = -geo.spanDeg / 2;
  cards.forEach((card, i) => {
    const deg = n === 1 ? 0 : start + (geo.spanDeg * i) / (n - 1);
    const rad = (deg * Math.PI) / 180;
    const h = nodeH(card, sizes);
    card.position = {
      x: cx + geo.radius * Math.sin(rad) - SIDE_CARD_W / 2,
      y: cy - geo.radius * Math.cos(rad) - h,
    };
  });
  return geo.radius;
}

/** 锚点右侧水平扇形（故事规则四卡） */
function placeFanRight(
  cards: LayoutNode[],
  anchor: LayoutNode,
  baseRadius: number,
  spanDeg = STORY_RULES_FAN_SPAN_DEG,
  sizes?: Map<string, LayoutSize>,
) {
  const n = cards.length;
  if (!n) return 0;
  const geo = resolveFanGeometry(n, baseRadius, spanDeg);
  const cx = anchor.position.x + ROOT_CARD_W;
  const cy = anchor.position.y;
  const start = -geo.spanDeg / 2;
  cards.forEach((card, i) => {
    const deg = n === 1 ? 0 : start + (geo.spanDeg * i) / (n - 1);
    const rad = (deg * Math.PI) / 180;
    const h = nodeH(card, sizes);
    card.position = {
      x: cx + geo.radius * Math.cos(rad) - SIDE_CARD_W / 2,
      y: cy + geo.radius * Math.sin(rad) - h / 2,
    };
  });
  return geo.radius;
}

function placeWorldviewFan(
  cards: LayoutNode[],
  root: LayoutNode,
  sizes?: Map<string, LayoutSize>,
  radius = WV_FAN_R,
  spanDeg = WV_FAN_SPAN_DEG,
): number {
  return placeFanAbove(cards, root, radius, sizes, spanDeg) ?? radius;
}

type WvLayoutRadii = {
  mainR?: number;
  mainSpanDeg?: number;
  axiomR?: number;
  locationR?: number;
  socialR?: number;
  historyR?: number;
  extSpanDeg?: number;
};

function extensionSpanCap(
  childCount: number,
  mainSpanDeg: number,
  mainCount: number,
  baseSpan = EXT_FAN_SPAN_DEG,
): number {
  if (childCount <= 1) return 0;
  const slotDeg = mainCount > 1 ? mainSpanDeg / (mainCount - 1) : mainSpanDeg;
  const maxSpan = Math.min(
    baseSpan,
    Math.max(MIN_EXT_FAN_SPAN_DEG, slotDeg * 0.85 * Math.max(1, childCount - 1) + 8),
  );
  return resolveFanGeometry(childCount, AXIOM_FAN_R, maxSpan).spanDeg;
}

function layoutWorldviewExtensions(
  fanBySlot: Map<string, LayoutNode>,
  axiomCards: LayoutNode[],
  locationCards: LayoutNode[],
  raceCards: LayoutNode[],
  factionCards: LayoutNode[],
  religionCards: LayoutNode[],
  majorEventCards: LayoutNode[],
  sizes?: Map<string, LayoutSize>,
  radii: WvLayoutRadii = {},
) {
  const mainSpan = radii.mainSpanDeg ?? WV_FAN_SPAN_DEG;
  const mainCount = fanCardsCount(fanBySlot);
  const extBase = radii.extSpanDeg ?? EXT_FAN_SPAN_DEG;
  const axR = radii.axiomR ?? AXIOM_FAN_R;
  const locR = radii.locationR ?? LOCATION_FAN_R;
  const socR = radii.socialR ?? SOCIAL_EXT_FAN_R;
  const histR = radii.historyR ?? SOCIAL_EXT_FAN_R;

  const coreLaws = fanBySlot.get("wv_core_laws");
  if (coreLaws && axiomCards.length) {
    const order = new Map((coreLaws.linked_knowledge_ids ?? []).map((id, i) => [id, i]));
    axiomCards.sort((a, b) => {
      const ia = order.has(a.id) ? order.get(a.id)! : 9999;
      const ib = order.has(b.id) ? order.get(b.id)! : 9999;
      return ia - ib || byY(a, b);
    });
    placeFanAbove(
      axiomCards,
      coreLaws,
      axR,
      sizes,
      extensionSpanCap(axiomCards.length, mainSpan, mainCount, extBase),
    );
  }
  const spatiotemporal = fanBySlot.get("wv_spatiotemporal");
  if (spatiotemporal && locationCards.length) {
    const order = new Map(
      (spatiotemporal.linked_knowledge_ids ?? []).map((id, i) => [id, i]),
    );
    locationCards.sort((a, b) => {
      const ia = order.has(a.id) ? order.get(a.id)! : 9999;
      const ib = order.has(b.id) ? order.get(b.id)! : 9999;
      return ia - ib || byY(a, b);
    });
    placeFanAbove(
      locationCards,
      spatiotemporal,
      locR,
      sizes,
      extensionSpanCap(locationCards.length, mainSpan, mainCount, extBase),
    );
  }
  const socialPower = fanBySlot.get("wv_social_power");
  if (socialPower && (raceCards.length || factionCards.length)) {
    const order = new Map(
      (socialPower.linked_knowledge_ids ?? []).map((id, i) => [id, i]),
    );
    const sortExt = (a: LayoutNode, b: LayoutNode) => {
      const ia = order.has(a.id) ? order.get(a.id)! : 9999;
      const ib = order.has(b.id) ? order.get(b.id)! : 9999;
      return ia - ib || byY(a, b);
    };
    raceCards.sort(sortExt);
    factionCards.sort(sortExt);
    const raceSpan = extensionSpanCap(raceCards.length, mainSpan, mainCount, extBase);
    const facSpan = extensionSpanCap(factionCards.length, mainSpan, mainCount, extBase);
    const both = raceCards.length > 0 && factionCards.length > 0;
    const fullSpan = Math.max(raceSpan, facSpan, extBase);
    const half = both ? (fullSpan - SOCIAL_HALF_GAP_DEG) / 2 : fullSpan / 2;
    const gapHalf = SOCIAL_HALF_GAP_DEG / 2;
    if (raceCards.length) {
      placeHalfFan(
        raceCards,
        socialPower,
        socR,
        both ? -fullSpan / 2 : -half,
        both ? -gapHalf : half,
        sizes,
      );
    }
    if (factionCards.length) {
      placeHalfFan(
        factionCards,
        socialPower,
        socR,
        both ? gapHalf : -half,
        both ? fullSpan / 2 : half,
        sizes,
      );
    }
  }
  const historyCulture = fanBySlot.get("wv_history_culture");
  if (historyCulture && (religionCards.length || majorEventCards.length)) {
    const order = new Map(
      (historyCulture.linked_knowledge_ids ?? []).map((id, i) => [id, i]),
    );
    const sortExt = (a: LayoutNode, b: LayoutNode) => {
      const ia = order.has(a.id) ? order.get(a.id)! : 9999;
      const ib = order.has(b.id) ? order.get(b.id)! : 9999;
      return ia - ib || byY(a, b);
    };
    religionCards.sort(sortExt);
    majorEventCards.sort(sortExt);
    const relSpan = extensionSpanCap(religionCards.length, mainSpan, mainCount, extBase);
    const evSpan = extensionSpanCap(majorEventCards.length, mainSpan, mainCount, extBase);
    const both = religionCards.length > 0 && majorEventCards.length > 0;
    const fullSpan = Math.max(relSpan, evSpan, extBase);
    const half = both ? (fullSpan - SOCIAL_HALF_GAP_DEG) / 2 : fullSpan / 2;
    const gapHalf = SOCIAL_HALF_GAP_DEG / 2;
    if (religionCards.length) {
      placeHalfFan(
        religionCards,
        historyCulture,
        histR,
        both ? -fullSpan / 2 : -half,
        both ? -gapHalf : half,
        sizes,
      );
    }
    if (majorEventCards.length) {
      placeHalfFan(
        majorEventCards,
        historyCulture,
        histR,
        both ? gapHalf : -half,
        both ? fullSpan / 2 : half,
        sizes,
      );
    }
  }
}

function fanCardsCount(fanBySlot: Map<string, LayoutNode>): number {
  return WV_FAN_ORDER.filter((s) => fanBySlot.has(s)).length;
}

function cardWidth(n: LayoutNode, sizes?: Map<string, LayoutSize>): number {
  const w = sizes?.get(n.id)?.w;
  return typeof w === "number" && w > 8 ? w : SIDE_CARD_W;
}

function cardCenterX(n: LayoutNode, sizes?: Map<string, LayoutSize>): number {
  return n.position.x + cardWidth(n, sizes) / 2;
}

/** 主卡 → 其子卡列表（按槽位） */
function childrenByMainSlot(
  axiomCards: LayoutNode[],
  locationCards: LayoutNode[],
  raceCards: LayoutNode[],
  factionCards: LayoutNode[],
  religionCards: LayoutNode[],
  majorEventCards: LayoutNode[],
): Map<string, LayoutNode[]> {
  return new Map([
    ["wv_core_laws", axiomCards],
    ["wv_spatiotemporal", locationCards],
    ["wv_social_power", [...raceCards, ...factionCards]],
    ["wv_history_culture", [...religionCards, ...majorEventCards]],
    ["wv_existence", []],
    ["wv_info_flow", []],
  ]);
}

/**
 * 主卡中心线向 dir（+1 右 / -1 左）到「自身+子卡」外缘的延伸距离。
 * 两主卡横向中心距至少应为：左卡右伸 + 右卡左伸 + 间隙。
 */
function reachFromCenter(
  main: LayoutNode,
  kids: LayoutNode[],
  dir: 1 | -1,
  sizes?: Map<string, LayoutSize>,
): number {
  const mc = cardCenterX(main, sizes);
  let reach = cardWidth(main, sizes) / 2;
  for (const k of kids) {
    const w = cardWidth(k, sizes);
    if (dir > 0) reach = Math.max(reach, k.position.x + w - mc);
    else reach = Math.max(reach, mc - k.position.x);
  }
  return reach;
}

/** 相邻主卡中心距是否小于「相对方向子卡外伸之和」 */
function adjacentMainsTooClose(
  fanBySlot: Map<string, LayoutNode>,
  kidsByMain: Map<string, LayoutNode[]>,
  sizes?: Map<string, LayoutSize>,
): boolean {
  const mains = WV_FAN_ORDER.map((s) => fanBySlot.get(s)).filter(
    (n): n is LayoutNode => !!n,
  );
  mains.sort(
    (a, b) =>
      cardCenterX(a, sizes) - cardCenterX(b, sizes) || a.id.localeCompare(b.id),
  );
  for (let i = 0; i < mains.length - 1; i++) {
    const left = mains[i]!;
    const right = mains[i + 1]!;
    const leftKids = kidsByMain.get(knowledgeSlotOf(left)) ?? [];
    const rightKids = kidsByMain.get(knowledgeSlotOf(right)) ?? [];
    const need =
      reachFromCenter(left, leftKids, 1, sizes) +
      reachFromCenter(right, rightKids, -1, sizes) +
      WV_CARD_GAP;
    const have = cardCenterX(right, sizes) - cardCenterX(left, sizes);
    if (have + 0.5 < need) return true;
  }
  return false;
}

function worldviewHasOverlap(nodes: LayoutNode[], sizes?: Map<string, LayoutSize>): boolean {
  const wv = nodes.filter(isWorldviewLayoutNode);
  for (let i = 0; i < wv.length; i++) {
    for (let j = i + 1; j < wv.length; j++) {
      if (fanRectsOverlap(fanBBox(wv[i]!, sizes), fanBBox(wv[j]!, sizes))) return true;
    }
  }
  return false;
}

/** 仍有重叠时微调位置（保扇形为主，必要时略横向错开） */
function nudgeWorldviewOverlaps(nodes: LayoutNode[], sizes?: Map<string, LayoutSize>) {
  const wv = nodes.filter(isWorldviewLayoutNode);
  for (let pass = 0; pass < 8; pass++) {
    let moved = false;
    for (let i = 0; i < wv.length; i++) {
      for (let j = i + 1; j < wv.length; j++) {
        const a = wv[i]!;
        const b = wv[j]!;
        const ba = fanBBox(a, sizes);
        const bb = fanBBox(b, sizes);
        if (!fanRectsOverlap(ba, bb, 0)) continue;
        const upper = a.position.y <= b.position.y ? a : b;
        const lower = upper === a ? b : a;
        const bu = fanBBox(upper, sizes);
        const bl = fanBBox(lower, sizes);
        const overlapY = bu.y + bu.h - bl.y;
        if (overlapY > 0) {
          upper.position.y -= overlapY + WV_CARD_GAP;
          moved = true;
        }
        const bu2 = fanBBox(upper, sizes);
        const bl2 = fanBBox(lower, sizes);
        if (fanRectsOverlap(bu2, bl2, 0)) {
          const pushX = (bu2.x + bu2.w - bl2.x) / 2 + WV_CARD_GAP / 2;
          if (pushX > 0 && bu2.x <= bl2.x) {
            upper.position.x -= pushX;
            lower.position.x += pushX;
            moved = true;
          }
        }
      }
    }
    if (!moved) break;
  }
}

function relayoutWorldview(
  fanCards: LayoutNode[],
  root: LayoutNode,
  fanBySlot: Map<string, LayoutNode>,
  axiomCards: LayoutNode[],
  locationCards: LayoutNode[],
  raceCards: LayoutNode[],
  factionCards: LayoutNode[],
  religionCards: LayoutNode[],
  majorEventCards: LayoutNode[],
  radii: WvLayoutRadii,
  sizes?: Map<string, LayoutSize>,
) {
  const mainGeo = resolveFanGeometry(
    fanCards.length,
    radii.mainR ?? WV_FAN_R,
    radii.mainSpanDeg ?? WV_FAN_SPAN_DEG,
  );
  placeWorldviewFan(fanCards, root, sizes, mainGeo.radius, mainGeo.spanDeg);
  layoutWorldviewExtensions(
    fanBySlot,
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
    sizes,
    {
      ...radii,
      mainR: mainGeo.radius,
      mainSpanDeg: mainGeo.spanDeg,
    },
  );
}

/**
 * 按「相邻主卡中心距 = 左卡右伸 + 右卡左伸 + 间隙」重排主卡 X，
 * 再把整组平移到根中心对齐。子卡相对偏移不变时，一次即可消交叉。
 */
function separateMainsByChildReach(
  fanBySlot: Map<string, LayoutNode>,
  kidsByMain: Map<string, LayoutNode[]>,
  root: LayoutNode,
  sizes?: Map<string, LayoutSize>,
): boolean {
  const mains = WV_FAN_ORDER.map((s) => fanBySlot.get(s)).filter(
    (n): n is LayoutNode => !!n,
  );
  if (mains.length < 2) return false;
  mains.sort(
    (a, b) =>
      cardCenterX(a, sizes) - cardCenterX(b, sizes) || a.id.localeCompare(b.id),
  );

  const reachR = mains.map((m) =>
    reachFromCenter(m, kidsByMain.get(knowledgeSlotOf(m)) ?? [], 1, sizes),
  );
  const reachL = mains.map((m) =>
    reachFromCenter(m, kidsByMain.get(knowledgeSlotOf(m)) ?? [], -1, sizes),
  );

  const targetCx: number[] = [cardCenterX(mains[0]!, sizes)];
  for (let i = 1; i < mains.length; i++) {
    targetCx.push(targetCx[i - 1]! + reachR[i - 1]! + reachL[i]! + WV_CARD_GAP);
  }

  // 平移使整组中心对齐根中心
  const midBefore =
    (targetCx[0]! + targetCx[targetCx.length - 1]!) / 2;
  const rootCx = root.position.x + ROOT_CARD_W / 2;
  const shift = rootCx - midBefore;
  for (let i = 0; i < targetCx.length; i++) targetCx[i]! += shift;

  let changed = false;
  for (let i = 0; i < mains.length; i++) {
    const m = mains[i]!;
    const w = cardWidth(m, sizes);
    const nx = targetCx[i]! - w / 2;
    if (Math.abs(nx - m.position.x) > 0.5) {
      m.position.x = nx;
      changed = true;
    }
  }
  return changed;
}

function placeWorldviewChildrenOnly(
  fanBySlot: Map<string, LayoutNode>,
  axiomCards: LayoutNode[],
  locationCards: LayoutNode[],
  raceCards: LayoutNode[],
  factionCards: LayoutNode[],
  religionCards: LayoutNode[],
  majorEventCards: LayoutNode[],
  radii: WvLayoutRadii,
  sizes?: Map<string, LayoutSize>,
) {
  const mainGeo = resolveFanGeometry(
    fanCardsCount(fanBySlot),
    radii.mainR ?? WV_FAN_R,
    radii.mainSpanDeg ?? WV_FAN_SPAN_DEG,
  );
  layoutWorldviewExtensions(
    fanBySlot,
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
    sizes,
    {
      ...radii,
      mainR: mainGeo.radius,
      mainSpanDeg: mainGeo.spanDeg,
    },
  );
}

/** 世界观主卡 + 子卡：先扇形，再按子卡外伸拉开相邻主卡横向间距 */
function fixWorldviewOverlaps(
  nodes: LayoutNode[],
  fanCards: LayoutNode[],
  root: LayoutNode,
  fanBySlot: Map<string, LayoutNode>,
  axiomCards: LayoutNode[],
  locationCards: LayoutNode[],
  raceCards: LayoutNode[],
  factionCards: LayoutNode[],
  religionCards: LayoutNode[],
  majorEventCards: LayoutNode[],
  sizes?: Map<string, LayoutSize>,
) {
  const wvNodes = nodes.filter(isWorldviewLayoutNode);
  if (wvNodes.length < 2 || fanCards.length === 0) return;

  const radii: WvLayoutRadii = {
    mainR: WV_FAN_R,
    mainSpanDeg: WV_FAN_SPAN_DEG,
    axiomR: AXIOM_FAN_R,
    locationR: LOCATION_FAN_R,
    socialR: SOCIAL_EXT_FAN_R,
    historyR: SOCIAL_EXT_FAN_R,
    extSpanDeg: EXT_FAN_SPAN_DEG,
  };

  const kidsByMain = childrenByMainSlot(
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
  );

  // 初始等角扇形 + 子扇
  relayoutWorldview(
    fanCards,
    root,
    fanBySlot,
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
    radii,
    sizes,
  );

  for (let pass = 0; pass < WV_FIX_MAX_PASSES; pass++) {
    // 核心：主卡横向间距 = 相对方向子卡外伸之和
    separateMainsByChildReach(fanBySlot, kidsByMain, root, sizes);
    placeWorldviewChildrenOnly(
      fanBySlot,
      axiomCards,
      locationCards,
      raceCards,
      factionCards,
      religionCards,
      majorEventCards,
      radii,
      sizes,
    );

    const tight = adjacentMainsTooClose(fanBySlot, kidsByMain, sizes);
    const hasOverlap = worldviewHasOverlap(nodes, sizes);
    if (!tight && !hasOverlap) break;

    // 同主卡子卡仍叠 → 拉高子扇半径、略收张角；不同主交叉则再走一轮间距分离即可
    let bumpAx = false;
    let bumpLoc = false;
    let bumpSoc = false;
    let bumpHist = false;
    for (let i = 0; i < wvNodes.length; i++) {
      for (let j = i + 1; j < wvNodes.length; j++) {
        const a = wvNodes[i]!;
        const b = wvNodes[j]!;
        if (!fanRectsOverlap(fanBBox(a, sizes), fanBBox(b, sizes))) continue;
        const sa = knowledgeSlotOf(a);
        const sb = knowledgeSlotOf(b);
        if (sa === "wv_axiom" && sb === "wv_axiom") bumpAx = true;
        if (sa === "wv_location" && sb === "wv_location") bumpLoc = true;
        if (
          (sa === "wv_race" || sa === "wv_faction") &&
          (sb === "wv_race" || sb === "wv_faction")
        ) {
          bumpSoc = true;
        }
        if (
          (sa === "wv_religion" || sa === "wv_major_event") &&
          (sb === "wv_religion" || sb === "wv_major_event")
        ) {
          bumpHist = true;
        }
      }
    }
    if (bumpAx) radii.axiomR = (radii.axiomR ?? AXIOM_FAN_R) + 24;
    if (bumpLoc) radii.locationR = (radii.locationR ?? LOCATION_FAN_R) + 24;
    if (bumpSoc) radii.socialR = (radii.socialR ?? SOCIAL_EXT_FAN_R) + 24;
    if (bumpHist) radii.historyR = (radii.historyR ?? SOCIAL_EXT_FAN_R) + 24;
    if (bumpAx || bumpLoc || bumpSoc || bumpHist) {
      radii.extSpanDeg = Math.max(
        MIN_EXT_FAN_SPAN_DEG,
        (radii.extSpanDeg ?? EXT_FAN_SPAN_DEG) - 3,
      );
    } else if (!tight) {
      break;
    }
  }

  // 最后再按外伸钉一次间距，避免子扇微调后回弹交叉
  separateMainsByChildReach(fanBySlot, kidsByMain, root, sizes);
  placeWorldviewChildrenOnly(
    fanBySlot,
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
    radii,
    sizes,
  );
  separateMainsByChildReach(fanBySlot, kidsByMain, root, sizes);
  placeWorldviewChildrenOnly(
    fanBySlot,
    axiomCards,
    locationCards,
    raceCards,
    factionCards,
    religionCards,
    majorEventCards,
    radii,
    sizes,
  );

  nudgeWorldviewOverlaps(nodes, sizes);
}

function placeStoryRulesCard(card: LayoutNode, root: LayoutNode) {
  card.position = { x: PLOT_X, y: root.position.y };
}

function placeStoryRulesFan(
  rules: LayoutNode,
  children: LayoutNode[],
  sizes?: Map<string, LayoutSize>,
) {
  if (!children.length) return 0;
  const order = new Map(
    ["sr_surface_setting", "sr_story_engine", "sr_fulfillment_system", "sr_constraint_redlines"].map(
      (s, i) => [s, i],
    ),
  );
  const sorted = children.slice().sort((a, b) => {
    const sa = order.get(knowledgeSlotOf(a)) ?? 99;
    const sb = order.get(knowledgeSlotOf(b)) ?? 99;
    return sa - sb || byY(a, b);
  });
  return placeFanRight(sorted, rules, STORY_RULES_FAN_R, STORY_RULES_FAN_SPAN_DEG, sizes) ?? 0;
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

  const fanBySlot = new Map<string, LayoutNode>();
  let storyRules: LayoutNode | undefined;
  const storyRulesFan: LayoutNode[] = [];
  const axiomCards: LayoutNode[] = [];
  const locationCards: LayoutNode[] = [];
  const raceCards: LayoutNode[] = [];
  const factionCards: LayoutNode[] = [];
  const religionCards: LayoutNode[] = [];
  const majorEventCards: LayoutNode[] = [];
  const writeHub = writePromptsHubOf(nodes);
  const writeGenKids = writeHub ? writePromptKids(writeHub, nodes, edges, "generate") : [];
  const writeRefineKids = writeHub ? writePromptKids(writeHub, nodes, edges, "refine") : [];
  const writeSkip = new Set<string>([
    ...(writeHub ? [writeHub.id] : []),
    ...writeGenKids.map((n) => n.id),
    ...writeRefineKids.map((n) => n.id),
  ]);
  for (const n of nodes.filter((n) => n.kind === "knowledge")) {
    const slot = knowledgeSlotOf(n);
    if (writeSkip.has(n.id) || slot === "write_prompts") {
      continue;
    }
    if (slot === "story_rules") {
      storyRules = n;
      continue;
    }
    if (
      slot === "sr_surface_setting" ||
      slot === "sr_story_engine" ||
      slot === "sr_fulfillment_system" ||
      slot === "sr_constraint_redlines"
    ) {
      storyRulesFan.push(n);
      continue;
    }
    if (slot === "wv_axiom") {
      axiomCards.push(n);
      continue;
    }
    if (slot === "wv_location") {
      locationCards.push(n);
      continue;
    }
    if (slot === "wv_race") {
      raceCards.push(n);
      continue;
    }
    if (slot === "wv_faction") {
      factionCards.push(n);
      continue;
    }
    if (slot === "wv_religion") {
      religionCards.push(n);
      continue;
    }
    if (slot === "wv_major_event") {
      majorEventCards.push(n);
      continue;
    }
    if (WV_FAN_ORDER.includes(slot)) {
      fanBySlot.set(slot, n);
      continue;
    }
    pushHosted(n, knowHost.get(n.id), knowsByHost, orphanKnows);
  }
  const fanCards = WV_FAN_ORDER.map((s) => fanBySlot.get(s)).filter(
    (n): n is LayoutNode => !!n,
  );

  for (const list of charsByHost.values()) list.sort(byY);
  for (const list of knowsByHost.values()) list.sort(byY);

  const occupiedPlots: { x: number; y: number }[] = [];

  // 根上有扇形 / 公理扇形时，主轴下移给上方留空
  let y = TOP;
  const mainFanGeo =
    fanCards.length > 0
      ? resolveFanGeometry(fanCards.length, WV_FAN_R, WV_FAN_SPAN_DEG)
      : null;
  const extFanGeo = (count: number, baseR: number) =>
    count > 0 ? resolveFanGeometry(count, baseR, EXT_FAN_SPAN_DEG) : null;
  if (mainFanGeo) {
    y += mainFanGeo.radius + defaultNodeH("knowledge") + SIDE_GAP;
  }
  const axGeo = extFanGeo(axiomCards.length, AXIOM_FAN_R);
  if (axGeo) y += axGeo.radius + defaultNodeH("knowledge") + SIDE_GAP;
  const locGeo = extFanGeo(locationCards.length, LOCATION_FAN_R);
  if (locGeo) y += locGeo.radius + defaultNodeH("knowledge") + SIDE_GAP;
  const raceGeo = extFanGeo(raceCards.length, SOCIAL_EXT_FAN_R);
  const facGeo = extFanGeo(factionCards.length, SOCIAL_EXT_FAN_R);
  const relGeo = extFanGeo(religionCards.length, SOCIAL_EXT_FAN_R);
  const evGeo = extFanGeo(majorEventCards.length, SOCIAL_EXT_FAN_R);
  if (raceGeo || facGeo) {
    y +=
      Math.max(raceGeo?.radius ?? 0, facGeo?.radius ?? 0) +
      defaultNodeH("knowledge") +
      SIDE_GAP;
  }
  if (relGeo || evGeo) {
    y +=
      Math.max(relGeo?.radius ?? 0, evGeo?.radius ?? 0) +
      defaultNodeH("knowledge") +
      SIDE_GAP;
  }
  for (const host of spine) {
    host.position = { x: MAIN_X, y };
    const plots = sortPlotsForHost(host, plotsByHost.get(host.id) ?? []);
    plotsByHost.set(host.id, plots);
    const chars = charsByHost.get(host.id) ?? [];
    const knows = knowsByHost.get(host.id) ?? [];

    if (host.kind === "novel") {
      fixWorldviewOverlaps(
        nodes,
        fanCards,
        host,
        fanBySlot,
        axiomCards,
        locationCards,
        raceCards,
        factionCards,
        religionCards,
        majorEventCards,
        sizes,
      );
      if (storyRules) {
        placeStoryRulesCard(storyRules, host);
        occupiedPlots.push({ ...storyRules.position });
        if (storyRulesFan.length) {
          placeStoryRulesFan(storyRules, storyRulesFan, sizes);
          for (const c of storyRulesFan) occupiedPlots.push({ ...c.position });
        }
      }
      if (writeHub) {
        writeHub.position = { x: WRITE_PROMPTS_HUB_X, y };
        occupiedPlots.push({ ...writeHub.position });
        placeSideByHeight(
          writeGenKids,
          WRITE_PROMPTS_HUB_X - SIDE_DX,
          y,
          SIDE_DX,
          MAX_SIDE_COL,
          -1,
          sizes,
        );
        placeSideByHeight(
          writeRefineKids,
          WRITE_PROMPTS_HUB_X + SIDE_DX,
          y,
          SIDE_DX,
          MAX_SIDE_COL,
          1,
          sizes,
        );
        for (const c of [...writeGenKids, ...writeRefineKids]) {
          occupiedPlots.push({ ...c.position });
        }
      }
    }

    const srFanReach =
      storyRules && storyRulesFan.length
        ? STORY_RULES_FAN_R + SIDE_CARD_W + PLOT_DX * 0.5
        : 0;
    const plotOriginX =
      host.kind === "novel" && storyRules ? PLOT_X + PLOT_DX + srFanReach : PLOT_X;
    placeGrid(plots, plotOriginX, y, PLOT_DX, PLOT_DY, MAX_PLOT_COL, 1);
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
    const knowSpan =
      host.kind === "novel" && writeHub
        ? placeSideByHeight(
            knows,
            (writeGenKids.length ? WRITE_PROMPTS_HUB_X - SIDE_DX : WRITE_PROMPTS_HUB_X) - SIDE_DX,
            y,
            SIDE_DX,
            MAX_SIDE_COL,
            -1,
            sizes,
          )
        : placeKnowLeftOfChars(knows, chars, y, sizes);
    const writeSpan =
      host.kind === "novel" && writeHub
        ? Math.max(
            defaultNodeH("knowledge"),
            writeGenKids.length ? writeGenKids.length * (defaultNodeH("knowledge") + SIDE_GAP) : 0,
            writeRefineKids.length
              ? writeRefineKids.length * (defaultNodeH("knowledge") + SIDE_GAP)
              : 0,
          )
        : 0;
    const storySpan =
      host.kind === "novel" && storyRules ? PLOT_DY : 0;
    const span = Math.max(
      gridSpan(plots.length, PLOT_DY, MAX_PLOT_COL),
      charSpan,
      knowSpan,
      storySpan,
      writeSpan,
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

/** 挂在该分卷 spine 下的章节 id（含经中间章节相连） */
export function volumeChapterIds(
  volumeId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  return nodes
    .filter((n) => n.kind === "chapter" && chapterParentVolumeId(n.id, nodes, edges) === volumeId)
    .map((n) => n.id);
}

/** 多个分卷时最多展开一个：返回应折叠的卷 id。expandId 指定展开哪张；null 表示全折。 */
export function accordionCollapsedVolumeIds(
  volumeIds: string[],
  collapsedStored: Iterable<string>,
  expandId?: string | null,
): string[] {
  if (volumeIds.length === 0) return [];
  if (expandId === null) return [...volumeIds];
  const stored = new Set(collapsedStored);
  const keep =
    typeof expandId === "string" && volumeIds.includes(expandId)
      ? expandId
      : (volumeIds.find((id) => !stored.has(id)) ?? null);
  if (!keep) return [...volumeIds];
  return volumeIds.filter((id) => id !== keep);
}

/** 折叠分卷时隐藏：卷下章节，以及只挂在这些章上的人物/剧情/知识卡 */
export function collapsedVolumeHiddenIds(
  collapsedVolumeIds: Iterable<string>,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): Set<string> {
  const hidden = new Set<string>();
  for (const vid of collapsedVolumeIds) {
    for (const cid of volumeChapterIds(vid, nodes, edges)) hidden.add(cid);
  }
  if (hidden.size === 0) return hidden;
  const byId = new Map(nodes.map((n) => [n.id, n]));
  for (const n of nodes) {
    if (n.kind === "novel" || n.kind === "volume" || n.kind === "chapter") continue;
    const hosts: string[] = [];
    for (const e of edges) {
      const other = e.target === n.id ? e.source : e.source === n.id ? e.target : null;
      if (!other) continue;
      const o = byId.get(other);
      if (!o) continue;
      if (o.kind === "novel" || o.kind === "volume" || o.kind === "chapter") hosts.push(o.id);
    }
    if (hosts.length && hosts.every((id) => hidden.has(id))) hidden.add(n.id);
  }
  return hidden;
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
  _chapterId: string,
  _nodes: LayoutNode[],
  _edges: LayoutEdge[],
): string[] {
  return [];
}

/** 章节直连知识卡 */
export function chapterLocalKnowledgeIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  const ch = nodes.find((n) => n.id === chapterId);
  return unionLinkedIds(
    nodes,
    edges,
    chapterId,
    ch?.linked_knowledge_ids ?? [],
    "knowledge",
  );
}

/** 章节面板：仅本章直连（知识不继承） */
export function chapterEffectiveKnowledgeIds(
  chapterId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  return chapterLocalKnowledgeIds(chapterId, nodes, edges);
}

/** 分卷直连知识 */
export function volumeLocalKnowledgeIds(
  volumeId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  return hostLinked(volumeId, nodes, edges, "linked_knowledge_ids", "knowledge");
}

export function volumeEffectiveKnowledgeIds(
  volumeId: string,
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): string[] {
  return volumeLocalKnowledgeIds(volumeId, nodes, edges);
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
  WV_FAN_R,
  WV_FAN_SPAN_DEG,
  AXIOM_FAN_R,
};
