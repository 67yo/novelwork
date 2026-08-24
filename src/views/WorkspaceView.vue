<script setup lang="ts">
import { computed, markRaw, nextTick, onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import {
  VueFlow,
  ConnectionMode,
  useVueFlow,
  type Connection,
  type Edge,
  type EdgeUpdateEvent,
  type Node,
  type NodeMouseEvent,
} from "@vue-flow/core";
import { Background } from "@vue-flow/background";
import { Controls } from "@vue-flow/controls";
import { MiniMap } from "@vue-flow/minimap";
import { useLocalStorage, useDebounceFn } from "@vueuse/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type ChapterMemoryGroup,
  type PublicKnowledgeCard,
  type NovelProject,
  type NovelTree,
  type TreeEdge,
  type TreeNode,
} from "@/lib/api";
import type { MessageKey } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Separator } from "@/components/ui/separator";
import StoryNode from "@/components/flow/StoryNode.vue";
import CharacterCardPanel from "@/components/CharacterCardPanel.vue";
import CoreLawsPanel from "@/components/CoreLawsPanel.vue";
import WorldAxiomPanel from "@/components/WorldAxiomPanel.vue";
import KeyLocationPanel from "@/components/KeyLocationPanel.vue";
import SocialPowerPanel from "@/components/SocialPowerPanel.vue";
import WorldRacePanel from "@/components/WorldRacePanel.vue";
import MajorFactionPanel from "@/components/MajorFactionPanel.vue";
import SpatiotemporalPanel from "@/components/SpatiotemporalPanel.vue";
import ExistencePanel from "@/components/ExistencePanel.vue";
import InfoFlowPanel from "@/components/InfoFlowPanel.vue";
import WorldviewChatPanel from "@/components/WorldviewChatPanel.vue";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";
import { useI18n } from "@/i18n";
import type { CoreLawsData, WorldAxiom } from "@/lib/coreLaws";
import { isAxiomSlot, normalizeAxiom } from "@/lib/coreLaws";
import {
  axiomHostCoreLawsId,
  axiomsFromLinkedCards,
  createAxiomCard,
  linkedAxiomCards,
  migrateInlineAxiomsToCards,
  syncCoreLawsExtracted,
} from "@/lib/axiomCards";
import type { KeyLocation, SpatiotemporalData } from "@/lib/spatiotemporal";
import { isLocationSlot, normalizeLocation } from "@/lib/spatiotemporal";
import {
  createLocationCard,
  linkedLocationCards,
  locationHostSpatiotemporalId,
  locationsFromLinkedCards,
  migrateInlineLocationsToCards,
  syncSpatiotemporalExtracted,
} from "@/lib/locationCards";
import {
  createFactionCard,
  createRaceCard,
  factionsFromLinkedCards,
  isSocialPowerChildSlot,
  linkedFactionCards,
  linkedRaceCards,
  migrateInlineSocialPowerToCards,
  racesFromLinkedCards,
  socialPowerHostId,
  syncSocialPowerExtracted,
} from "@/lib/socialPowerCards";
import type { MajorFaction, SocialPowerData, WorldRace } from "@/lib/socialPower";
import type { ExistenceData } from "@/lib/existence";
import type { InfoFlowData } from "@/lib/infoFlow";
import {
  isFactionSlot,
  isRaceSlot,
  normalizeFaction,
  normalizeRace,
} from "@/lib/socialPower";
import {
  BookOpen,
  Brain,
  Copy,
  GitBranch,
  Layers,
  LayoutGrid,
  Library,
  ListTree,
  Loader2,
  PanelBottom,
  PanelRight,
  Plus,
  RefreshCw,
  Share2,
  Sparkles,
  Square,
  Trash2,
  User,
  X,
} from "@lucide/vue";
import { diffLines } from "@/lib/linediff";
import { renderChapterMd, stripChapterMeta } from "@/lib/md";
import { replaceBodyLine, splitBodyLines } from "@/lib/chapterParagraphs";
import {
  applyAutoLayout,
  chapterEffectiveKnowledgeIds,
  chapterInheritedKnowledgeIds,
  chapterInheritedPlotIds,
  chapterLocalPlotIds,
  chapterParentVolumeId,
  rootLinkedKnowledgeIds,
  rootLinkedPlotIds,
  volumeLocalPlotIds,
  volumeEffectiveKnowledgeIds,
} from "@/lib/treeLayout";
import {
  ensureWorldviewCards,
  isFixedRootKnowledgeEdge,
  isFixedRootKnowledgeSlot,
  isWorldviewFanSlot,
  knowledgeSlot,
  missingWorldviewSlots,
  worldviewComplete,
  worldviewFanTitle,
} from "@/lib/worldview";
import {
  buildKnowledgeNavItems,
  filterHostPanelLinkedKnowledge,
  knowledgeChipShell,
  knowledgeNodeLabel,
  sortLinkedKnowledgeNodes,
} from "@/lib/knowledgeListSort";

const props = defineProps<{ id: string }>();
const { t, locale } = useI18n();
const { fitView, setNodes, setEdges, removeNodes, findNode } = useVueFlow({
  id: "nove-workspace",
});

/** 禁用 Vue Flow 自带 Delete（只改画布不落盘）；改由 onCanvasKeydown 走后端删除。 */
const flowDeleteKeyCode = null as null;

const novel = ref<NovelProject | null>(null);
const tree = ref<NovelTree | null>(null);
const selected = ref<TreeNode | null>(null);
const publicKnowledgeCards = ref<PublicKnowledgeCard[]>([]);
const publicPickOpen = ref(false);
const publicPickFilter = ref("");
const publicPickBusy = ref(false);
const publishBusy = ref(false);
const chapterMd = ref("");
/** 章节正文手动编辑 */
const bodyEditing = ref(false);
const bodyDraft = ref("");
const bodySaveBusy = ref(false);
const bodyAutosaved = ref(false);
const bodyTaEl = ref<HTMLTextAreaElement | null>(null);
const bodyMirrorEl = ref<HTMLElement | null>(null);
const bodyScrollTop = ref(0);
const paraGutterTops = ref<{ index: number; top: number; bottom: number }[]>([]);
/** 鼠标悬停段落；仅该段显示 AI 改写 icon */
const hoveredParaIndex = ref<number | null>(null);
const pendingParaRewrite = ref<{ index: number; text: string } | null>(null);
const paraRewriteNote = ref("");
const paraRewriteBusy = ref(false);
const paraRewriteError = ref("");
/** 上次成功落盘的正文快照；用于跳过无变更的自动保存 */
let lastSavedBody = "";
const prevN = ref(10);
/** 画布浮动导航显隐（章节 / 人物 / 剧情 / 知识） */
const showChapterNav = useLocalStorage("novework.showChapterNav", true);
type CanvasNavTab = "chapter" | "volume" | "character" | "plot" | "knowledge";
const canvasNavTab = useLocalStorage<CanvasNavTab>("novework.canvasNavTab", "chapter");
/** memory = 章节记忆+大纲；full = 前序章正文（跨会话记住上次选择） */
const refineMode = useLocalStorage<"memory" | "full">("novework.refineMode", "memory");
const memoryPanelOpen = ref(false);
const memoryItems = ref<string[]>([]);
const memoryBusy = ref(false);
const memoryError = ref("");
/** 抽取注意事项：用户改过即作为默认（跨会话）；空则打开时填入出厂缺省 */
const memoryExtractNotes = useLocalStorage("novework.memoryExtractNotes", "");
/** 记忆面板：勾选其他章记忆作抽取对照 */
const memoryRefPicks = ref<{ node_id: string; label: string; has_memory: boolean }[]>([]);
const memoryRefSelectedIds = ref<string[]>([]);
const memoryRefLoading = ref(false);
/** 全书章节记忆面板 */
const allMemoryOpen = ref(false);
const allMemoryGroups = ref<ChapterMemoryGroup[]>([]);
const allMemoryBusy = ref(false);
const allMemoryError = ref("");
const allMemorySelectedId = ref("");
const allMemoryBody = ref("");
const allMemoryBodyBusy = ref(false);
/** null itemIndex = 清除整章记忆 */
const pendingMemoryDelete = ref<{
  nodeId: string;
  label: string;
  itemIndex: number | null;
} | null>(null);
const deletingMemory = ref(false);
const busy = ref("");
const notice = ref("");
/** 预生成/精修等待：当前阶段与进度 */
const chapterProgress = ref<{ step: string; index: number; total: number } | null>(null);
/** 当前 AI 请求已发送（prompt）token；confirmed=API 实值，否则为估算 */
const chapterTokens = ref<{
  prompt: number;
  completion: number;
  confirmed: boolean;
} | null>(null);
/** 每步耗时（最后一步 done=false 时为进行中） */
const chapterStepTimings = ref<{ step: string; ms: number; done: boolean }[]>([]);
let chapterStepStartedAt = 0;
let chapterTickTimer: ReturnType<typeof setInterval> | null = null;
/** 驱动进行中步骤的秒表刷新 */
const chapterTick = ref(0);
let unlistenChapterProgress: (() => void) | null = null;
let unlistenChapterTokens: (() => void) | null = null;
let unlistenTreeChanged: (() => void) | null = null;
/** 全章自动：预生成 → 精修，可点按钮中止 */
/** 预生成/精修结束后的结束语（左侧底部独立区） */
const chapterResultNotice = ref("");
const coverBusy = ref(false);
const coverPromptBusy = ref(false);
const coverPrompt = ref("");
const coverPromptHint = ref("");
const copyHint = ref("");
/** 精修前后快照（会话内，按节点） */
const refineBeforeByNode = ref<Record<string, string>>({});
const bodyTab = ref<"body" | "diff">("body");

const nodeTypes = { story: markRaw(StoryNode) };
const flowNodes = ref<Node[]>([]) as Ref<Node[]>;
const flowEdges = ref<Edge[]>([]) as Ref<Edge[]>;

const coverUrl = computed(() =>
  novel.value?.cover_path ? convertFileSrc(novel.value.cover_path) : "",
);

function cloneTreeNodeData(n: TreeNode): TreeNode {
  return {
    ...n,
    character: n.character ? { ...n.character } : null,
    knowledge: n.knowledge
      ? {
          ...n.knowledge,
          book_ids: [...(n.knowledge.book_ids ?? [])],
        }
      : null,
    side_plot: n.side_plot ? { ...n.side_plot } : null,
    linked_character_ids: [...(n.linked_character_ids ?? [])],
    linked_side_plot_ids: [...(n.linked_side_plot_ids ?? [])],
    linked_knowledge_ids: [...(n.linked_knowledge_ids ?? [])],
    position: { ...n.position },
  };
}

/** 丢掉指向已删节点的边与 linked_*；章节剥离根/分卷继承剧情（只读，不参与排序）。 */
function pruneTreeRefs(tr: NovelTree) {
  const alive = new Set(tr.nodes.map((n) => n.id));
  tr.edges = tr.edges.filter((e) => alive.has(e.source) && alive.has(e.target));
  for (const n of tr.nodes) {
    n.linked_character_ids = (n.linked_character_ids ?? []).filter((id) => alive.has(id));
    n.linked_side_plot_ids = (n.linked_side_plot_ids ?? []).filter((id) => alive.has(id));
    n.linked_knowledge_ids = (n.linked_knowledge_ids ?? []).filter((id) => alive.has(id));
    if (n.kind === "chapter") {
      const inherited = new Set(chapterInheritedPlotIds(n.id, tr.nodes, tr.edges));
      n.linked_side_plot_ids = n.linked_side_plot_ids.filter((id) => !inherited.has(id));
    } else if (n.kind === "volume") {
      const rootSet = new Set(rootLinkedPlotIds(tr.nodes, tr.edges));
      n.linked_side_plot_ids = n.linked_side_plot_ids.filter((id) => !rootSet.has(id));
    }
  }
}

function syncFlowFromTree() {
  const tr = tree.value;
  if (!tr) {
    flowNodes.value = [];
    flowEdges.value = [];
    setNodes([]);
    setEdges([]);
    return;
  }
  const nodes = tr.nodes.map((n) => {
    const base = cloneTreeNodeData(n);
    const data =
      n.kind === "novel" && novel.value
        ? {
            ...base,
            word_count_min: base.word_count_min || novel.value.word_count_min,
            word_count_max: base.word_count_max || novel.value.word_count_max,
            chapter_count: base.chapter_count || novel.value.chapter_count,
            cover_url: novel.value.cover_path
              ? convertFileSrc(novel.value.cover_path)
              : "",
          }
        : base;
    return {
      id: n.id,
      type: "story" as const,
      position: { ...n.position },
      data,
      selected: n.id === selected.value?.id,
      // 禁止 Vue Flow 内置删除；统一走 deleteTreeCard
      deletable: false,
    };
  });
  const edges = tr.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    sourceHandle: e.source_handle ?? edgeDefaultSource(e.kind),
    targetHandle: e.target_handle ?? edgeDefaultTarget(e.kind),
    // ponytail: animated edges keep WebKit.GPU busy at idle; use color/style instead
    animated: false,
    // 世界观 / 故事规则连线不可拖改、不可双击切断
    updatable: !isFixedRootKnowledgeEdge(tr.nodes, e),
    label: edgeDisplayLabel(e),
    style: edgeStyle(e.kind),
  }));
  // 用 setNodes/setEdges 全量替换，避免 v-model 合并导致已删节点「复活」
  setNodes(nodes);
  setEdges(edges);
  flowNodes.value = nodes;
  flowEdges.value = edges;
}

function nodeKind(id: string) {
  return tree.value?.nodes.find((n) => n.id === id)?.kind;
}

function isCharCharEdge(e: { source: string; target: string }) {
  return nodeKind(e.source) === "character" && nodeKind(e.target) === "character";
}

function edgeDisplayLabel(e: TreeEdge) {
  if (isCharCharEdge(e)) return e.label?.trim() || t("workspace.relationHint");
  if (e.kind === "character") return t("workspace.role");
  if (e.kind === "side_plot") return t("workspace.plot");
  if (e.kind === "knowledge") return t("workspace.knowledgeCard");
  return "";
}

function edgeDefaultSource(kind: string) {
  if (kind === "character") return "left";
  if (kind === "side_plot") return "right";
  if (kind === "knowledge") return "left";
  return "bottom";
}

function edgeDefaultTarget(kind: string) {
  if (kind === "character") return "right";
  if (kind === "side_plot") return "left";
  if (kind === "knowledge") return "right";
  return "top";
}

function edgeStyle(kind: string): Record<string, string> {
  if (kind === "character") return { stroke: "#d97706" };
  if (kind === "side_plot") return { stroke: "#0284c7" };
  if (kind === "knowledge") return { stroke: "#0f766e" };
  return { stroke: "#3d6b4f" };
}

function kindFromNodes(sourceId: string, targetId: string): string {
  const a = nodeKind(sourceId);
  const b = nodeKind(targetId);
  if (!a || !b) return "chapter";
  if (a === "character" && b === "character") return "character";
  const hasChar = a === "character" || b === "character";
  const hasPlot = a === "side_plot" || b === "side_plot";
  const hasKnowledge = a === "knowledge" || b === "knowledge";
  const hasChapter = a === "chapter" || b === "chapter";
  const hasNovel = a === "novel" || b === "novel";
  const hasVolume = a === "volume" || b === "volume";
  const host = hasChapter || hasNovel || hasVolume;
  if (hasKnowledge && host) return "knowledge";
  if (hasChar && (hasPlot || host)) return "character";
  if (hasPlot && host) return "side_plot";
  if (a === "volume" && b === "volume") return "volume";
  if (hasVolume && (hasNovel || hasChapter)) return hasNovel && hasVolume ? "volume" : "chapter";
  return "chapter";
}

async function loadAll() {
  novel.value = await api.getNovel(props.id);
  let tr = await api.getTree(props.id);
  tr = await maybeMigrateWorldview(tr);
  tree.value = tr;
  publicKnowledgeCards.value = await api.listPublicKnowledgeCards().catch(() => []);
  syncFlowFromTree();
  const root = tree.value.nodes.find((n) => n.kind === "novel");
  const firstChapter = tree.value.nodes.find((n) => n.kind === "chapter");
  if (firstChapter) await selectNode(firstChapter);
  else if (root) await selectNode(root);
  await nextTick();
  // Vue Flow 节点尺寸就绪后再 fit，默认完整展示当前树
  setTimeout(() => {
    void fitView({ padding: 0.2, duration: 320 });
  }, 80);
}

async function reloadTreeFromDisk() {
  let tr = await api.getTree(props.id);
  tr = await maybeMigrateWorldview(tr);
  tree.value = tr;
  if (selected.value) {
    const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
    if (n) selected.value = n;
  }
  syncFlowFromTree();
}

async function maybeMigrateWorldview(tr: NovelTree): Promise<NovelTree> {
  const ax = migrateInlineAxiomsToCards(tr);
  const loc = migrateInlineLocationsToCards(tr);
  const sp = migrateInlineSocialPowerToCards(tr);
  if (!ax && !loc && !sp) return tr;
  applyAutoLayout(tr.nodes, tr.edges);
  return persistTree(tr);
}

function markFlowSelection() {
  const id = selected.value?.id;
  flowNodes.value = flowNodes.value.map((n) => ({
    ...n,
    selected: n.id === id,
  }));
}

async function selectNode(n: TreeNode) {
  selected.value = n;
  markFlowSelection();
  void api.setWorkspaceSelection(props.id, n.id);
  bodyTab.value = "body";
  notice.value = "";
  chapterResultNotice.value = "";
  copyHint.value = "";
  memoryPanelOpen.value = false;
  pendingParaRewrite.value = null;
  if (n.kind === "chapter" || n.kind === "side_plot") {
    chapterMd.value = await api.getChapter(props.id, n.id);
  } else {
    chapterMd.value = "";
  }
  if (n.kind === "chapter") {
    enterChapterBodyEdit();
  } else {
    leaveChapterBodyEdit();
  }
}

/** 选中章节后始终进入全文编辑（无 Markdown 预览模式） */
function enterChapterBodyEdit() {
  if (!selected.value || selected.value.kind !== "chapter") {
    leaveChapterBodyEdit();
    return;
  }
  bodyDraft.value = stripChapterMeta(chapterMd.value);
  bodyEditing.value = true;
  bodyAutosaved.value = false;
  lastSavedBody = bodyDraft.value;
  bodyTab.value = bodyTab.value === "diff" ? bodyTab.value : "body";
  nextTick(() => refreshParaGutters());
}

function leaveChapterBodyEdit() {
  bodyEditing.value = false;
  bodyDraft.value = "";
  bodyAutosaved.value = false;
  lastSavedBody = "";
  paraGutterTops.value = [];
  hoveredParaIndex.value = null;
  pendingParaRewrite.value = null;
}

/** 生成/精修前：关掉段落改写弹层，保留编辑态 */
function cancelEditChapterBody() {
  pendingParaRewrite.value = null;
}

const bodyLines = computed(() => splitBodyLines(bodyDraft.value));

const refreshParaGutters = useDebounceFn(() => {
  const mirror = bodyMirrorEl.value;
  const ta = bodyTaEl.value;
  if (!mirror || !ta || !bodyEditing.value) {
    paraGutterTops.value = [];
    return;
  }
  mirror.style.width = `${ta.clientWidth}px`;
  const spans = mirror.querySelectorAll<HTMLElement>("[data-para-i]");
  const tops: { index: number; top: number; bottom: number }[] = [];
  for (const el of spans) {
    if (el.dataset.nonempty !== "1") continue;
    const i = Number(el.dataset.paraI);
    if (!Number.isFinite(i)) continue;
    const top = el.offsetTop;
    tops.push({ index: i, top, bottom: top + el.offsetHeight });
  }
  paraGutterTops.value = tops;
}, 40);

function onBodyTaScroll() {
  bodyScrollTop.value = bodyTaEl.value?.scrollTop ?? 0;
}

function onBodyEditorPointerMove(ev: PointerEvent) {
  const ta = bodyTaEl.value;
  if (!ta || !bodyEditing.value || paraRewriteBusy.value) {
    hoveredParaIndex.value = null;
    return;
  }
  const rect = ta.getBoundingClientRect();
  const y = ev.clientY - rect.top + ta.scrollTop;
  let hit: number | null = null;
  for (const g of paraGutterTops.value) {
    if (y >= g.top && y < g.bottom) {
      hit = g.index;
      break;
    }
  }
  hoveredParaIndex.value = hit;
}

function onBodyEditorPointerLeave() {
  hoveredParaIndex.value = null;
}


async function persistChapterBody() {
  if (!selected.value || selected.value.kind !== "chapter" || bodySaveBusy.value) return;
  const nodeId = selected.value.id;
  const draft = bodyDraft.value;
  if (draft === lastSavedBody) return;
  bodySaveBusy.value = true;
  try {
    const words = await api.saveChapter(props.id, nodeId, draft);
    lastSavedBody = draft;
    chapterMd.value = draft.trim() ? `${draft.trim()}\n` : "";
    bodyAutosaved.value = true;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const cur = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (cur) selected.value = cur;
    }
    chapterResultNotice.value = t("workspace.bodySaved", { n: words });
  } catch (e) {
    chapterResultNotice.value = String(e);
    bodyAutosaved.value = false;
  } finally {
    bodySaveBusy.value = false;
    if (bodyEditing.value && bodyDraft.value !== lastSavedBody) {
      scheduleBodyAutosave();
    }
  }
}

const scheduleBodyAutosave = useDebounceFn(() => {
  if (!bodyEditing.value) return;
  void persistChapterBody();
}, 3000);

watch(bodyDraft, () => {
  if (!bodyEditing.value) return;
  bodyAutosaved.value = false;
  scheduleBodyAutosave();
  nextTick(() => refreshParaGutters());
});


function openParaRewrite(index: number) {
  const line = bodyLines.value[index];
  if (!line?.trim() || paraRewriteBusy.value) return;
  pendingParaRewrite.value = { index, text: line };
  paraRewriteNote.value = "";
  paraRewriteError.value = "";
}

function cancelParaRewrite() {
  if (paraRewriteBusy.value) return;
  pendingParaRewrite.value = null;
  paraRewriteNote.value = "";
  paraRewriteError.value = "";
}

async function confirmParaRewrite() {
  const pending = pendingParaRewrite.value;
  const note = paraRewriteNote.value.trim();
  if (!pending || !note || paraRewriteBusy.value) return;
  paraRewriteBusy.value = true;
  paraRewriteError.value = "";
  try {
    const out = await api.rewriteChapterParagraph(props.id, pending.text, note);
    bodyDraft.value = replaceBodyLine(bodyDraft.value, pending.index, out);
    pendingParaRewrite.value = null;
    paraRewriteNote.value = "";
    await persistChapterBody();
    nextTick(() => refreshParaGutters());
  } catch (e) {
    paraRewriteError.value = String(e);
  } finally {
    paraRewriteBusy.value = false;
  }
}


function onNodeClick(ev: NodeMouseEvent) {
  const data = ev.node.data as TreeNode;
  if (data) void selectNode(data);
}

function unlinkEdgeRefs(e: TreeEdge) {
  if (!tree.value) return;
  if (isFixedRootKnowledgeEdge(tree.value.nodes, e)) return;
  const src = tree.value.nodes.find((n) => n.id === e.source);
  const tgt = tree.value.nodes.find((n) => n.id === e.target);
  if (!src || !tgt) return;
  if (src.kind === "character" && tgt.kind === "character") return;
  if (e.kind === "character") {
    const charId = src.kind === "character" ? src.id : tgt.kind === "character" ? tgt.id : null;
    const host = src.kind === "character" ? tgt : src;
    if (charId) host.linked_character_ids = host.linked_character_ids.filter((id) => id !== charId);
  }
  if (e.kind === "side_plot") {
    const plotId = src.kind === "side_plot" ? src.id : tgt.kind === "side_plot" ? tgt.id : null;
    const host = src.kind === "side_plot" ? tgt : src;
    if (plotId) host.linked_side_plot_ids = host.linked_side_plot_ids.filter((id) => id !== plotId);
  }
  if (e.kind === "knowledge") {
    const kid = src.kind === "knowledge" ? src.id : tgt.kind === "knowledge" ? tgt.id : null;
    const host = src.kind === "knowledge" ? tgt : src;
    if (kid) host.linked_knowledge_ids = (host.linked_knowledge_ids ?? []).filter((id) => id !== kid);
  }
}

function linkEdgeRefs(e: TreeEdge) {
  if (!tree.value) return;
  const src = tree.value.nodes.find((n) => n.id === e.source);
  const tgt = tree.value.nodes.find((n) => n.id === e.target);
  if (!src || !tgt) return;
  if (src.kind === "character" && tgt.kind === "character") return;
  if (e.kind === "character") {
    const char = src.kind === "character" ? src : tgt.kind === "character" ? tgt : null;
    const host = src.kind === "character" ? tgt : src;
    if (char && !host.linked_character_ids.includes(char.id)) {
      host.linked_character_ids.push(char.id);
    }
  }
  if (e.kind === "side_plot") {
    const plot = src.kind === "side_plot" ? src : tgt.kind === "side_plot" ? tgt : null;
    const host = src.kind === "side_plot" ? tgt : src;
    if (!plot) return;
    // 章/卷：继承自根（或章继承自卷）的剧情不写进本机列表，只靠继承展示
    if (host.kind === "chapter") {
      const inherited = new Set(
        chapterInheritedPlotIds(host.id, tree.value.nodes, tree.value.edges),
      );
      if (inherited.has(plot.id)) return;
    } else if (host.kind === "volume") {
      const rootSet = new Set(rootLinkedPlotIds(tree.value.nodes, tree.value.edges));
      if (rootSet.has(plot.id)) return;
    }
    if (!host.linked_side_plot_ids.includes(plot.id)) {
      host.linked_side_plot_ids.push(plot.id);
    }
  }
  if (e.kind === "knowledge") {
    const k = src.kind === "knowledge" ? src : tgt.kind === "knowledge" ? tgt : null;
    const host = src.kind === "knowledge" ? tgt : src;
    host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
    if (k && !host.linked_knowledge_ids.includes(k.id)) {
      host.linked_knowledge_ids.push(k.id);
    }
  }
}

function linkedNotice(kind: string) {
  notice.value = t("workspace.linkedNotice", {
    kind:
      kind === "character"
        ? t("workspace.role")
        : kind === "side_plot"
          ? t("workspace.plotCard")
          : kind === "knowledge"
            ? t("workspace.knowledgeCard")
            : t("workspace.plot"),
  });
}

const relationEdgeId = ref<string | null>(null);
const relationDraft = ref("");
const relationInputEl = ref<HTMLInputElement | null>(null);

const relationPairLabel = computed(() => {
  if (!tree.value || !relationEdgeId.value) return "";
  const e = tree.value.edges.find((x) => x.id === relationEdgeId.value);
  if (!e) return "";
  const a = tree.value.nodes.find((n) => n.id === e.source)?.label ?? "";
  const b = tree.value.nodes.find((n) => n.id === e.target)?.label ?? "";
  return `${a} ↔ ${b}`;
});

function openRelationEditor(edge: TreeEdge) {
  relationEdgeId.value = edge.id;
  relationDraft.value = edge.label ?? "";
  nextTick(() => relationInputEl.value?.focus());
}

function closeRelationEditor() {
  relationEdgeId.value = null;
  relationDraft.value = "";
}

async function saveRelation() {
  if (!tree.value || !relationEdgeId.value) return;
  const te = tree.value.edges.find((e) => e.id === relationEdgeId.value);
  if (!te) {
    closeRelationEditor();
    return;
  }
  te.label = relationDraft.value.trim();
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  closeRelationEditor();
}

async function onConnect(conn: Connection) {
  if (!tree.value || !conn.source || !conn.target) return;
  const kind = kindFromNodes(conn.source, conn.target);
  const edge: TreeEdge = {
    id: `e-${conn.source}-${conn.target}-${conn.sourceHandle ?? ""}-${Date.now()}`,
    source: conn.source,
    target: conn.target,
    kind,
    source_handle: conn.sourceHandle,
    target_handle: conn.targetHandle,
    label: "",
  };
  if (tree.value.edges.some((e) => e.source === edge.source && e.target === edge.target && e.kind === edge.kind)) {
    return;
  }
  tree.value.edges.push(edge);
  linkEdgeRefs(edge);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  if (isCharCharEdge(edge)) openRelationEditor(edge);
  else linkedNotice(kind);
}

async function onEdgeUpdate(ev: EdgeUpdateEvent) {
  if (!tree.value || !ev.connection.source || !ev.connection.target) return;
  const te = tree.value.edges.find((e) => e.id === ev.edge.id);
  if (!te) return;
  if (isFixedRootKnowledgeEdge(tree.value.nodes, te)) {
    notice.value = t("workspace.wv.linkLocked");
    syncFlowFromTree();
    return;
  }
  unlinkEdgeRefs(te);
  const kind = kindFromNodes(ev.connection.source, ev.connection.target);
  te.source = ev.connection.source;
  te.target = ev.connection.target;
  te.source_handle = ev.connection.sourceHandle;
  te.target_handle = ev.connection.targetHandle;
  te.kind = kind;
  if (!isCharCharEdge(te)) te.label = "";
  linkEdgeRefs(te);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  linkedNotice(kind);
}

function onEdgeClick(ev: { edge: { id: string } }) {
  if (!tree.value) return;
  const te = tree.value.edges.find((e) => e.id === ev.edge.id);
  if (!te || !isCharCharEdge(te)) return;
  openRelationEditor(te);
}

async function onEdgeDoubleClick(ev: { edge: { id: string } }) {
  if (!tree.value) return;
  const te = tree.value.edges.find((e) => e.id === ev.edge.id);
  if (!te) return;
  if (isFixedRootKnowledgeEdge(tree.value.nodes, te)) {
    notice.value = t("workspace.wv.linkLocked");
    return;
  }
  if (relationEdgeId.value === te.id) closeRelationEditor();
  unlinkEdgeRefs(te);
  tree.value.edges = tree.value.edges.filter((e) => e.id !== te.id);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  notice.value = t("workspace.edgeRemoved");
}

const CHAPTER_STEPS: Record<string, MessageKey> = {
  context: "workspace.taskStepContext",
  writing: "workspace.taskStepWriting",
  refining: "workspace.taskStepRefining",
  check_beats: "workspace.taskStepCheckBeats",
  repair_land: "workspace.taskStepRepairLand",
  repair_beats: "workspace.taskStepCheckBeats",
  repair_length: "workspace.taskStepRepairLength",
  memory: "workspace.taskStepMemory",
  planning: "workspace.taskStepPlanning",
  gen_plots: "workspace.taskStepGenPlots",
  saving: "workspace.taskStepSaving",
};

function chapterStepLabel(step: string): string {
  const key = CHAPTER_STEPS[step];
  return key ? t(key) : step;
}

function formatStepMs(ms: number): string {
  const sec = Math.max(0, ms) / 1000;
  if (sec < 60) return `${sec.toFixed(1)}s`;
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function startChapterTick() {
  if (chapterTickTimer) return;
  chapterTick.value = 0;
  chapterTickTimer = setInterval(() => {
    chapterTick.value = performance.now();
  }, 200);
}

function stopChapterTick() {
  if (chapterTickTimer) {
    clearInterval(chapterTickTimer);
    chapterTickTimer = null;
  }
}

function beginChapterTask(step: string, index: number, total: number) {
  stopChapterTick();
  chapterStepStartedAt = performance.now();
  chapterProgress.value = { step, index, total };
  chapterTokens.value = null;
  chapterStepTimings.value = [{ step, ms: 0, done: false }];
  startChapterTick();
}

function applyChapterProgress(step: string, index: number, total: number) {
  const now = performance.now();
  const list = [...chapterStepTimings.value];
  if (list.length) {
    const last = list[list.length - 1];
    if (!last.done && last.step === step) {
      chapterProgress.value = { step, index, total };
      return;
    }
    if (!last.done) {
      last.ms = Math.round(now - chapterStepStartedAt);
      last.done = true;
    }
  }
  list.push({ step, ms: 0, done: false });
  chapterStepTimings.value = list;
  chapterStepStartedAt = now;
  chapterProgress.value = { step, index, total };
  startChapterTick();
}

function endChapterTask() {
  const now = performance.now();
  const list = [...chapterStepTimings.value];
  if (list.length) {
    const last = list[list.length - 1];
    if (!last.done) {
      last.ms = Math.round(now - chapterStepStartedAt);
      last.done = true;
      chapterStepTimings.value = list;
    }
  }
  stopChapterTick();
}

function clearChapterTaskUi() {
  endChapterTask();
  chapterProgress.value = null;
  chapterTokens.value = null;
  chapterStepTimings.value = [];
}

const chapterProgressLabel = computed(() => {
  const p = chapterProgress.value;
  if (!p) return "";
  return chapterStepLabel(p.step);
});

const chapterProgressPct = computed(() => {
  const p = chapterProgress.value;
  if (!p || p.total <= 0) return 0;
  return Math.min(100, Math.round((p.index / p.total) * 100));
});

/** 含进行中步骤的实时耗时列表 */
const chapterStepTimingsView = computed(() => {
  void chapterTick.value;
  const now = performance.now();
  return chapterStepTimings.value.map((s) => ({
    step: s.step,
    label: chapterStepLabel(s.step),
    done: s.done,
    ms: s.done ? s.ms : Math.round(now - chapterStepStartedAt),
  }));
});

const chapterTotalMs = computed(() =>
  chapterStepTimingsView.value.reduce((sum, s) => sum + s.ms, 0),
);

watch(busy, (v) => {
  if (
    !v ||
    (v !== "generate" &&
      v !== "refine" &&
      v !== "plan-next" &&
      v !== "gen-plots" &&
      v !== "regen-memory" &&
      v !== "gen-cards")
  ) {
    clearChapterTaskUi();
  }
});


onMounted(async () => {
  await loadAll();
  unlistenChapterProgress = await listen<{
    novelId: string;
    step: string;
    index: number;
    total: number;
  }>("chapter-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    applyChapterProgress(ev.payload.step, ev.payload.index, ev.payload.total);
  });
  unlistenChapterTokens = await listen<{
    novelId: string;
    promptTokens: number;
    completionTokens: number;
    confirmed: boolean;
  }>("chapter-tokens", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    chapterTokens.value = {
      prompt: ev.payload.promptTokens,
      completion: ev.payload.completionTokens,
      confirmed: ev.payload.confirmed,
    };
  });
  unlistenTreeChanged = await listen<{ novelId: string }>("novel-tree-changed", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    void reloadTreeFromDisk();
  });
  window.addEventListener("resize", refreshParaGutters);
});
onUnmounted(() => {
  unlistenChapterProgress?.();
  unlistenChapterProgress = null;
  unlistenChapterTokens?.();
  unlistenChapterTokens = null;
  unlistenTreeChanged?.();
  unlistenTreeChanged = null;
  window.removeEventListener("resize", refreshParaGutters);
  stopChapterTick();
  void api.setWorkspaceSelection(null, null);
});
watch(
  () => props.id,
  async () => {
    await loadAll();
  },
);
watch(locale, () => {
  if (tree.value) {
    for (const n of tree.value.nodes) {
      if (n.kind !== "knowledge") continue;
      const slot = knowledgeSlot(n);
      if (isWorldviewFanSlot(slot)) applyWorldviewFanLabel(n, slot);
    }
  }
  syncFlowFromTree();
});

function isCancelledErr(e: unknown): boolean {
  return String(e).toLowerCase().includes("cancelled");
}

const generateBrief = useLocalStorage("novework.generateBrief", "");

function factoryGenerateBrief(): string {
  return t("workspace.generateBriefDefault");
}

function ensureGenerateBrief() {
  if (!generateBrief.value.trim()) {
    generateBrief.value = factoryGenerateBrief();
  }
}

async function executeGenerate(
  nodeId: string,
  memoryNodeIds: string[],
  userBrief: string,
): Promise<boolean> {
  cancelEditChapterBody();
  busy.value = "generate";
  beginChapterTask("context", 1, 6);
  notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.generateChapter(props.id, nodeId, memoryNodeIds, userBrief);
    delete refineBeforeByNode.value[nodeId];
    refineBeforeByNode.value = { ...refineBeforeByNode.value };
    chapterMd.value = r.content;
    bodyTab.value = "body";
    enterChapterBodyEdit();
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    return true;
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
    return false;
  } finally {
    endChapterTask();
    busy.value = "";
  }
}

/** 自动全章：跳过确认框，默认不带历史记忆 + 已存条件（靠进行中剧情卡导航） */
async function generate(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  ensureGenerateBrief();
  return executeGenerate(selected.value.id, [], generateBrief.value);
}

// ponytail: auto-gen UI 暂未挂回；保留函数用于后续接回按钮。
void generate;

function factoryMemoryExtractNotes(): string {
  return t("workspace.memoryExtractNotesDefault");
}

function ensureMemoryExtractNotes() {
  if (!memoryExtractNotes.value.trim()) {
    memoryExtractNotes.value = factoryMemoryExtractNotes();
  }
}

function resetMemoryExtractNotes() {
  if (busy.value === "regen-memory") return;
  memoryExtractNotes.value = factoryMemoryExtractNotes();
}

function toggleMemoryRefId(id: string) {
  if (busy.value === "regen-memory") return;
  const set = new Set(memoryRefSelectedIds.value);
  if (set.has(id)) set.delete(id);
  else set.add(id);
  memoryRefSelectedIds.value = [...set];
}

function selectAllMemoryRefs() {
  if (busy.value === "regen-memory" || !memoryRefPicks.value.length) return;
  memoryRefSelectedIds.value = memoryRefPicks.value.map((c) => c.node_id);
}

function deselectAllMemoryRefs() {
  if (busy.value === "regen-memory") return;
  memoryRefSelectedIds.value = [];
}

async function loadMemoryRefPicks(nodeId: string) {
  memoryRefLoading.value = true;
  memoryRefPicks.value = [];
  memoryRefSelectedIds.value = [];
  try {
    const chapters = (tree.value?.nodes ?? [])
      .filter((n) => n.kind === "chapter" && n.id !== nodeId)
      .slice()
      .sort((a, b) => a.position.y - b.position.y || a.position.x - b.position.x);
    const groups = await api.listAllChapterMemory(props.id);
    const has = new Set(groups.filter((g) => g.items.length).map((g) => g.node_id));
    const picks = chapters.map((c) => ({
      node_id: c.id,
      label: c.label,
      has_memory: has.has(c.id),
    }));
    memoryRefPicks.value = picks;
    // 默认勾选已有记忆的其他章
    memoryRefSelectedIds.value = picks.filter((p) => p.has_memory).map((p) => p.node_id);
  } catch {
    memoryRefPicks.value = [];
    memoryRefSelectedIds.value = [];
  } finally {
    memoryRefLoading.value = false;
  }
}

async function openChapterMemory() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  memoryPanelOpen.value = true;
  ensureMemoryExtractNotes();
  memoryBusy.value = true;
  memoryError.value = "";
  memoryItems.value = [];
  const nodeId = selected.value.id;
  void loadMemoryRefPicks(nodeId);
  try {
    memoryItems.value = await api.getChapterMemory(props.id, nodeId);
  } catch (e) {
    memoryError.value = String(e);
  } finally {
    memoryBusy.value = false;
  }
}

function closeChapterMemory() {
  if (busy.value === "regen-memory") return;
  memoryPanelOpen.value = false;
  memoryRefPicks.value = [];
  memoryRefSelectedIds.value = [];
}

async function regenerateChapterMemory() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value) return;
  if (!chapterMd.value.trim()) {
    memoryError.value = t("workspace.noBody");
    return;
  }
  memoryPanelOpen.value = true;
  busy.value = "regen-memory";
  beginChapterTask("memory", 1, 2);
  notice.value = "";
  chapterResultNotice.value = "";
  memoryError.value = "";
  try {
    const items = await api.regenerateChapterMemory(
      props.id,
      selected.value.id,
      memoryExtractNotes.value,
      memoryRefSelectedIds.value,
    );
    memoryItems.value = items;
    chapterResultNotice.value = t("workspace.regenMemoryDone", { n: items.length });
  } catch (e) {
    const msg = String(e);
    if (!msg.includes("cancelled")) memoryError.value = msg;
  } finally {
    endChapterTask();
    busy.value = "";
  }
}

async function openAllChapterMemory() {
  allMemoryOpen.value = true;
  allMemoryBusy.value = true;
  allMemoryError.value = "";
  allMemoryGroups.value = [];
  allMemorySelectedId.value = "";
  allMemoryBody.value = "";
  try {
    allMemoryGroups.value = await api.listAllChapterMemory(props.id);
    if (allMemoryGroups.value.length) {
      await selectAllMemoryChapter(allMemoryGroups.value[0].node_id);
    }
  } catch (e) {
    allMemoryError.value = String(e);
  } finally {
    allMemoryBusy.value = false;
  }
}

function closeAllChapterMemory() {
  allMemoryOpen.value = false;
  pendingMemoryDelete.value = null;
}

async function selectAllMemoryChapter(nodeId: string) {
  allMemorySelectedId.value = nodeId;
  allMemoryBodyBusy.value = true;
  allMemoryBody.value = "";
  try {
    allMemoryBody.value = await api.getChapter(props.id, nodeId);
  } catch (e) {
    allMemoryBody.value = String(e);
  } finally {
    allMemoryBodyBusy.value = false;
  }
}

const allMemorySelectedGroup = computed(() =>
  allMemoryGroups.value.find((g) => g.node_id === allMemorySelectedId.value) ?? null,
);

function askDeleteMemoryItem(itemIndex: number) {
  const g = allMemorySelectedGroup.value;
  if (!g || deletingMemory.value) return;
  pendingMemoryDelete.value = {
    nodeId: g.node_id,
    label: g.label,
    itemIndex,
  };
}

function askClearChapterMemory() {
  const g = allMemorySelectedGroup.value;
  if (!g || deletingMemory.value) return;
  pendingMemoryDelete.value = {
    nodeId: g.node_id,
    label: g.label,
    itemIndex: null,
  };
}

function cancelPendingMemoryDelete() {
  if (deletingMemory.value) return;
  pendingMemoryDelete.value = null;
}

async function confirmPendingMemoryDelete() {
  const p = pendingMemoryDelete.value;
  if (!p || deletingMemory.value) return;
  const group = allMemoryGroups.value.find((g) => g.node_id === p.nodeId);
  if (!group) {
    pendingMemoryDelete.value = null;
    return;
  }
  const next =
    p.itemIndex === null
      ? []
      : group.items.filter((_, i) => i !== p.itemIndex);
  deletingMemory.value = true;
  try {
    await api.setChapterMemory(props.id, p.nodeId, next);
    if (next.length === 0) {
      allMemoryGroups.value = allMemoryGroups.value.filter((g) => g.node_id !== p.nodeId);
      if (allMemorySelectedId.value === p.nodeId) {
        const first = allMemoryGroups.value[0];
        if (first) await selectAllMemoryChapter(first.node_id);
        else {
          allMemorySelectedId.value = "";
          allMemoryBody.value = "";
        }
      }
    } else {
      allMemoryGroups.value = allMemoryGroups.value.map((g) =>
        g.node_id === p.nodeId ? { ...g, items: next } : g,
      );
    }
    pendingMemoryDelete.value = null;
  } catch (e) {
    allMemoryError.value = String(e);
    pendingMemoryDelete.value = null;
  } finally {
    deletingMemory.value = false;
  }
}

/** 精修条件：用户改过即作为默认（跨会话）；空则打开时填入出厂缺省 */
const refineBrief = useLocalStorage("novework.refineBrief", "");

function factoryRefineBrief(): string {
  return t("workspace.refineBriefDefault");
}

function ensureRefineBrief() {
  if (!refineBrief.value.trim()) {
    refineBrief.value = factoryRefineBrief();
  }
}

async function executeRefine(nodeId: string, userBrief: string): Promise<boolean> {
  cancelEditChapterBody();
  busy.value = "refine";
  beginChapterTask("context", 1, 2);
  notice.value = "";
  chapterResultNotice.value = "";
  const before = chapterMd.value;
  try {
    const r = await api.refineChapter(
      props.id,
      nodeId,
      prevN.value,
      refineMode.value,
      userBrief,
    );
    refineBeforeByNode.value = { ...refineBeforeByNode.value, [nodeId]: before };
    chapterMd.value = r.content;
    bodyTab.value = "diff";
    enterChapterBodyEdit();
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    return true;
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
    return false;
  } finally {
    endChapterTask();
    busy.value = "";
  }
}

/** 自动全章：跳过确认框，用已存精修条件 */
async function refine(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  ensureRefineBrief();
  return executeRefine(selected.value.id, refineBrief.value);
}

// ponytail: auto-refine UI 暂未挂回；保留函数用于后续接回按钮。
void refine;

async function stopChapter() {
  if (!chapterBusy.value) return;
  try {
    await api.chatCancel(props.id);
  } catch {
    /* ignore */
  }
}

async function copyChapterBody() {
  const md = stripChapterMeta(bodyEditing.value ? bodyDraft.value : chapterMd.value);
  if (!md) return;
  const el = document.createElement("div");
  el.innerHTML = renderChapterMd(md);
  let text = (el.innerText || md).trim();
  // 正文里没有标题时，补上章节卡标题
  const title = selected.value?.kind === "chapter" ? selected.value.label.trim() : "";
  if (title && !/^#{1,6}\s/.test(md)) {
    text = `${title}\n\n${text}`;
  }
  try {
    await navigator.clipboard.writeText(text.trimEnd() + "\n");
    copyHint.value = t("workspace.copied");
    window.setTimeout(() => {
      if (copyHint.value === t("workspace.copied")) copyHint.value = "";
    }, 2000);
  } catch {
    copyHint.value = t("workspace.copyFailed");
  }
}

function emptyCharacter(): NonNullable<TreeNode["character"]> {
  return {
    role: "",
    personality: "",
    motto: "",
    gender: "",
    style: "",
    alignment: "",
    age: "",
    constraints: "",
  };
}

function emptyKnowledge(): NonNullable<TreeNode["knowledge"]> {
  return {
    book_ids: [],
    extract_prompt: "",
    extracted: "",
    slot: "",
    core_laws: null,
    spatiotemporal: null,
    world_axiom: null,
    key_location: null,
    social_power: null,
    world_race: null,
    major_faction: null,
    existence: null,
    info_flow: null,
  };
}

function plainTree(tr: NovelTree): NovelTree {
  return JSON.parse(JSON.stringify(tr)) as NovelTree;
}

async function persistTree(tr: NovelTree) {
  const plain = plainTree(tr);
  // 保证写入路径与当前小说一致
  plain.novel_id = props.id;
  await api.saveTree(plain);
  return plain;
}

/** 左侧编辑栏拖拽排序 → 写回 linked_side_plot_ids（仅本机剧情；根/分卷继承不参与） */
async function reorderHostPlots(
  hostId: string,
  hostKind: "chapter" | "volume",
  plotIds: string[],
) {
  if (!tree.value) return;
  const node = tree.value.nodes.find((n) => n.id === hostId && n.kind === hostKind);
  if (!node) return;
  const inherited = new Set(
    hostKind === "chapter"
      ? [
          ...rootLinkedPlotIds(tree.value.nodes, tree.value.edges),
          ...(() => {
            const vid = chapterParentVolumeId(hostId, tree.value!.nodes, tree.value!.edges);
            return vid
              ? volumeLocalPlotIds(vid, tree.value!.nodes, tree.value!.edges)
              : [];
          })(),
        ]
      : rootLinkedPlotIds(tree.value.nodes, tree.value.edges),
  );
  const next = plotIds.filter((id) => !inherited.has(id));
  const prev = (node.linked_side_plot_ids ?? []).filter((id) => !inherited.has(id));
  if (prev.length === next.length && prev.every((id, i) => id === next[i])) return;
  node.linked_side_plot_ids = next;
  if (selected.value?.id === hostId) {
    selected.value = { ...selected.value, linked_side_plot_ids: [...next] };
  }
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  if (selected.value?.id === hostId) {
    const cur = tree.value.nodes.find((x) => x.id === hostId);
    if (cur) selected.value = cur;
  }
}

async function autoLayout() {
  if (!tree.value) return;
  // 从 Vue Flow 取最新坐标 + 实测尺寸（人物卡高度不一，避免叠住）
  const layoutSizes = new Map<string, { w: number; h: number }>();
  for (const n of tree.value.nodes) {
    const gn = findNode(n.id);
    if (!gn) continue;
    n.position = { x: gn.position.x, y: gn.position.y };
    const w = gn.dimensions?.width ?? 0;
    const h = gn.dimensions?.height ?? 0;
    if (w > 8 && h > 8) layoutSizes.set(n.id, { w, h });
  }
  pruneTreeRefs(tree.value);
  applyAutoLayout(tree.value.nodes, tree.value.edges, layoutSizes);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function addCard(kind: "chapter" | "volume" | "character" | "side_plot" | "knowledge") {
  if (!tree.value) return;
  const root = tree.value.nodes.find((n) => n.kind === "novel");
  if (!root) return;
  const id = crypto.randomUUID();
  const maxY = Math.max(...tree.value.nodes.map((n) => n.position.y), 0);
  let node: TreeNode;
  let edge: TreeEdge;

  if (kind === "volume") {
    const vols = tree.value.nodes.filter((n) => n.kind === "volume").length;
    const sel = selected.value;
    const prev = sel?.kind === "volume" ? sel : root;
    node = {
      id,
      kind,
      label: t("workspace.volumeN", { n: vols + 1 }),
      outline: "",
      character: null,
      knowledge: null,
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: prev.position.x, y: maxY + 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    edge = {
      id: `e-${prev.id}-${id}`,
      source: prev.id,
      target: id,
      kind: "volume",
      source_handle: "bottom",
      target_handle: "top",
    };
  } else if (kind === "chapter") {
    const chapters = tree.value.nodes
      .filter((n) => n.kind === "chapter")
      .sort((a, b) => a.position.y - b.position.y);
    const sel = selected.value;
    let prev = chapters.length ? chapters[chapters.length - 1]! : root;
    if (sel?.kind === "volume") {
      const under = chapters.filter(
        (c) => chapterParentVolumeId(c.id, tree.value!.nodes, tree.value!.edges) === sel.id,
      );
      prev = under.length ? under[under.length - 1]! : sel;
    } else if (sel?.kind === "chapter") {
      const vid = chapterParentVolumeId(sel.id, tree.value.nodes, tree.value.edges);
      if (vid) {
        const under = chapters.filter(
          (c) => chapterParentVolumeId(c.id, tree.value!.nodes, tree.value!.edges) === vid,
        );
        prev = under.length ? under[under.length - 1]! : sel;
      } else {
        prev = sel;
      }
    }
    const num = chapters.length + 1;
    node = {
      id,
      kind,
      label: t("workspace.chapterN", { n: num }),
      outline: "",
      character: null,
      knowledge: null,
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: root.position.x, y: maxY + 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    edge = {
      id: `e-${prev.id}-${id}`,
      source: prev.id,
      target: id,
      kind: "chapter",
      source_handle: "bottom",
      target_handle: "top",
    };
  } else if (kind === "character") {
    const host =
      selected.value &&
      (selected.value.kind === "novel" ||
        selected.value.kind === "volume" ||
        selected.value.kind === "chapter" ||
        selected.value.kind === "side_plot")
        ? selected.value
        : root;
    const chars = tree.value.nodes.filter((n) => n.kind === "character").length;
    node = {
      id,
      kind,
      label: t("workspace.newCharacter"),
      outline: "",
      character: emptyCharacter(),
      knowledge: null,
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: 40, y: 80 + chars * 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    host.linked_character_ids.push(id);
    edge = {
      id: `e-${host.id}-${id}`,
      source: host.id,
      target: id,
      kind: "character",
      source_handle: "left",
      target_handle: "right",
    };
  } else if (kind === "knowledge") {
    const host =
      selected.value &&
      (selected.value.kind === "novel" ||
        selected.value.kind === "volume" ||
        selected.value.kind === "chapter" ||
        selected.value.kind === "side_plot")
        ? selected.value
        : root;
    const kn = tree.value.nodes.filter((n) => n.kind === "knowledge").length;
    const chars = tree.value.nodes.filter((n) => n.kind === "character").length;
    node = {
      id,
      kind,
      label: t("workspace.newKnowledge"),
      outline: "",
      character: null,
      knowledge: emptyKnowledge(),
      side_plot: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: 40, y: 80 + (chars + kn) * 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    host.linked_knowledge_ids = host.linked_knowledge_ids ?? [];
    host.linked_knowledge_ids.push(id);
    edge = {
      id: `e-${host.id}-${id}`,
      source: host.id,
      target: id,
      kind: "knowledge",
      source_handle: "left",
      target_handle: "right",
    };
  } else {
    // 默认挂到当前选中章/卷/根；选中剧情卡时挂到该剧情所挂的宿主
    let host = root;
    const sel = selected.value;
    if (sel?.kind === "novel" || sel?.kind === "volume" || sel?.kind === "chapter") {
      host = sel;
    } else if (sel?.kind === "side_plot") {
      const owner = tree.value.nodes.find(
        (n) =>
          (n.kind === "chapter" || n.kind === "novel" || n.kind === "volume") &&
          (n.linked_side_plot_ids ?? []).includes(sel.id),
      );
      if (owner) host = owner;
    }
    const plots = tree.value.nodes.filter((n) => n.kind === "side_plot").length;
    node = {
      id,
      kind,
      label: t("workspace.newPlot"),
      outline: "",
      character: null,
      knowledge: null,
      side_plot: { status: "active", absorbed: false },
      linked_character_ids: [],
      linked_side_plot_ids: [],
      linked_knowledge_ids: [],
      position: { x: root.position.x + 280, y: 80 + plots * 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    host.linked_side_plot_ids.push(id);
    edge = {
      id: `e-${host.id}-${id}`,
      source: host.id,
      target: id,
      kind: "side_plot",
      source_handle: "right",
      target_handle: "left",
    };
  }

  tree.value.nodes.push(node);
  tree.value.edges.push(edge);
  pruneTreeRefs(tree.value);
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function pickAndSetCover() {
  coverBusy.value = true;
  try {
    const path = await api.pickCover();
    if (path) {
      novel.value = await api.setCover(props.id, path);
      syncFlowFromTree();
    }
  } catch (e) {
    notice.value = String(e);
  } finally {
    coverBusy.value = false;
  }
}

async function generateCoverPrompt() {
  if (coverPromptBusy.value) return;
  coverPromptBusy.value = true;
  coverPromptHint.value = "";
  try {
    coverPrompt.value = await api.generateCoverPrompt(props.id);
  } catch (e) {
    notice.value = String(e);
  } finally {
    coverPromptBusy.value = false;
  }
}

async function copyCoverPrompt() {
  const text = coverPrompt.value.trim();
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    coverPromptHint.value = t("workspace.coverPromptCopied");
    window.setTimeout(() => {
      if (coverPromptHint.value === t("workspace.coverPromptCopied")) coverPromptHint.value = "";
    }, 2000);
  } catch {
    coverPromptHint.value = t("workspace.copyFailed");
  }
}

const selectedIsChapter = computed(() => selected.value?.kind === "chapter");
const selectedIsCharacter = computed(() => selected.value?.kind === "character");
const selectedIsKnowledge = computed(() => selected.value?.kind === "knowledge");
const selectedIsCoreLaws = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_core_laws",
);
const selectedIsWorldAxiom = computed(
  () =>
    selected.value?.kind === "knowledge" && isAxiomSlot(knowledgeSlot(selected.value)),
);
const selectedIsKeyLocation = computed(
  () =>
    selected.value?.kind === "knowledge" && isLocationSlot(knowledgeSlot(selected.value)),
);
const selectedIsSocialPower = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_social_power",
);
const selectedIsWorldRace = computed(
  () => selected.value?.kind === "knowledge" && isRaceSlot(knowledgeSlot(selected.value)),
);
const selectedIsMajorFaction = computed(
  () => selected.value?.kind === "knowledge" && isFactionSlot(knowledgeSlot(selected.value)),
);
const selectedIsSpatiotemporal = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_spatiotemporal",
);
const selectedIsExistence = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_existence",
);
const selectedIsInfoFlow = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_info_flow",
);
const selectedWorldviewFanSlot = computed(() => {
  if (!selected.value || selected.value.kind !== "knowledge") return "";
  const slot = knowledgeSlot(selected.value);
  return isWorldviewFanSlot(slot) ? slot : "";
});
const selectedIsNovel = computed(() => selected.value?.kind === "novel");

const coreLawsAxiomLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsCoreLaws.value) return [];
  return linkedAxiomCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title: n.knowledge?.world_axiom?.name?.trim() || n.label || "",
  }));
});

const coreLawsLinkedAxioms = computed((): WorldAxiom[] => {
  if (!tree.value || !selected.value || !selectedIsCoreLaws.value) return [];
  return axiomsFromLinkedCards(tree.value, selected.value.id);
});

const spatiotemporalLocationLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsSpatiotemporal.value) return [];
  return linkedLocationCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title: n.knowledge?.key_location?.name?.trim() || n.label || "",
  }));
});

const spatiotemporalLinkedLocations = computed((): KeyLocation[] => {
  if (!tree.value || !selected.value || !selectedIsSpatiotemporal.value) return [];
  return locationsFromLinkedCards(tree.value, selected.value.id);
});

const socialPowerRaceLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return [];
  return linkedRaceCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title: n.knowledge?.world_race?.name?.trim() || n.label || "",
  }));
});

const socialPowerFactionLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return [];
  return linkedFactionCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title:
      n.knowledge?.major_faction?.name?.trim() ||
      n.knowledge?.major_faction?.faction_type?.trim() ||
      n.label ||
      "",
  }));
});

const socialPowerLinkedRaces = computed((): WorldRace[] => {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return [];
  return racesFromLinkedCards(tree.value, selected.value.id);
});

const socialPowerLinkedFactions = computed((): MajorFaction[] => {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return [];
  return factionsFromLinkedCards(tree.value, selected.value.id);
});
const selectedIsVolume = computed(() => selected.value?.kind === "volume");

const rootWorldviewComplete = computed(() =>
  tree.value ? worldviewComplete(tree.value) : false,
);
const worldviewBusy = ref(false);
const worldviewChatOpen = ref(false);

async function openWorldviewChat() {
  if (!tree.value || worldviewBusy.value) return;
  worldviewBusy.value = true;
  try {
    const before = missingWorldviewSlots(tree.value).length;
    ensureWorldviewCards(tree.value, (key) => t(key as Parameters<typeof t>[0]));
    migrateInlineAxiomsToCards(tree.value);
    migrateInlineLocationsToCards(tree.value);
    migrateInlineSocialPowerToCards(tree.value);
    applyAutoLayout(tree.value.nodes, tree.value.edges);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
    if (before > 0) {
      await nextTick();
      void fitView({ padding: 0.18, duration: 280 });
    }
    worldviewChatOpen.value = true;
  } catch (e) {
    notice.value = String(e);
  } finally {
    worldviewBusy.value = false;
  }
}

async function onWorldviewChatApplied() {
  if (!tree.value) return;
  try {
    let tr = await api.getTree(props.id);
    tr = await maybeMigrateWorldview(tr);
    tree.value = tr;
    syncFlowFromTree();
    if (selected.value?.kind === "novel") {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    notice.value = t("workspace.wv.chatDone");
    await nextTick();
    void fitView({ padding: 0.18, duration: 280 });
  } catch (e) {
    notice.value = String(e);
  }
}

function sortNodesByCanvasPosition<T extends { position: { x: number; y: number } }>(nodes: T[]): T[] {
  return nodes.slice().sort((a, b) => a.position.y - b.position.y || a.position.x - b.position.x);
}

type CanvasNavItem = { id: string; label: string; kind: TreeNode["kind"]; indent?: number };

/** 根 → 章节（按画布 y 排序），供左侧浮动导航 */
const canvasNavChapterItems = computed((): CanvasNavItem[] => {
  const tr = tree.value;
  if (!tr) return [];
  const root = tr.nodes.find((n) => n.kind === "novel");
  const chapters = sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "chapter"));
  const items: CanvasNavItem[] = [];
  if (root) {
    items.push({
      id: root.id,
      label: root.label?.trim() || novel.value?.title || t("workspace.chapterNavRoot"),
      kind: "novel",
    });
  }
  for (const ch of chapters) {
    items.push({ id: ch.id, label: ch.label?.trim() || t("workspace.newChapter"), kind: "chapter" });
  }
  return items;
});

const canvasNavVolumeItems = computed((): CanvasNavItem[] => {
  const tr = tree.value;
  if (!tr) return [];
  return sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "volume")).map((n) => ({
    id: n.id,
    label: n.label?.trim() || t("workspace.volumeBadge"),
    kind: "volume" as const,
  }));
});

const canvasNavCharacterItems = computed((): CanvasNavItem[] => {
  const tr = tree.value;
  if (!tr) return [];
  return sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "character")).map((n) => ({
    id: n.id,
    label: n.label?.trim() || t("workspace.role"),
    kind: "character" as const,
  }));
});

const canvasNavPlotItems = computed((): CanvasNavItem[] => {
  const tr = tree.value;
  if (!tr) return [];
  return sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "side_plot")).map((n) => ({
    id: n.id,
    label: n.label?.trim() || t("workspace.plot"),
    kind: "side_plot" as const,
  }));
});

const canvasNavKnowledgeItems = computed((): CanvasNavItem[] => {
  const tr = tree.value;
  if (!tr) return [];
  return buildKnowledgeNavItems(tr, t);
});

const canvasNavTabDefs = computed(() =>
  [
    { id: "chapter" as const, label: t("workspace.canvasNavTabChapter"), icon: BookOpen },
    { id: "volume" as const, label: t("workspace.canvasNavTabVolume"), icon: Layers },
    { id: "character" as const, label: t("workspace.canvasNavTabCharacter"), icon: User },
    { id: "plot" as const, label: t("workspace.canvasNavTabPlot"), icon: GitBranch },
    { id: "knowledge" as const, label: t("workspace.canvasNavTabKnowledge"), icon: Library },
  ] satisfies { id: CanvasNavTab; label: string; icon: typeof BookOpen }[],
);

const canvasNavActiveItems = computed((): CanvasNavItem[] => {
  switch (canvasNavTab.value) {
    case "volume":
      return canvasNavVolumeItems.value;
    case "character":
      return canvasNavCharacterItems.value;
    case "plot":
      return canvasNavPlotItems.value;
    case "knowledge":
      return canvasNavKnowledgeItems.value;
    default:
      return canvasNavChapterItems.value;
  }
});

const canvasNavEmptyLabel = computed(() => {
  switch (canvasNavTab.value) {
    case "volume":
      return t("workspace.canvasNavEmptyVolume");
    case "character":
      return t("workspace.canvasNavEmptyCharacter");
    case "plot":
      return t("workspace.canvasNavEmptyPlot");
    case "knowledge":
      return t("workspace.canvasNavEmptyKnowledge");
    default:
      return t("workspace.canvasNavEmptyChapter");
  }
});

async function focusCanvasNav(id: string) {
  const tr = tree.value;
  if (!tr) return;
  const n = tr.nodes.find((x) => x.id === id);
  if (!n) return;
  await selectNode(n);
  await nextTick();
  void fitView({ nodes: [id], padding: 0.45, duration: 280, maxZoom: 1.25 });
}

function toggleChapterNav() {
  showChapterNav.value = !showChapterNav.value;
}

const canDeleteSelectedCard = computed(() => {
  const n = selected.value;
  if (!n || n.kind === "novel") return false;
  if (n.kind === "knowledge" && isFixedRootKnowledgeSlot(knowledgeSlot(n))) return false;
  return true;
});

type PendingCardDelete = {
  id: string;
  title: string;
  hasBody: boolean;
  isVolume: boolean;
  step: 1 | 2;
};
const pendingCardDelete = ref<PendingCardDelete | null>(null);
const deletingCard = ref(false);

async function chapterHasGeneratedBody(nodeId: string, wordCount: number): Promise<boolean> {
  if (wordCount > 0) return true;
  try {
    const body = await api.getChapter(props.id, nodeId);
    return stripChapterMeta(body).trim().length > 0;
  } catch {
    return false;
  }
}

/** 删除非根节点：后端原子删节点+边+linked_*，再刷新画布。根节点无删除入口。 */
async function deleteSelectedCard() {
  if (!tree.value || !selected.value || selected.value.kind === "novel") return;
  if (!canDeleteSelectedCard.value || deletingCard.value) return;
  const id = selected.value.id;
  const title = selected.value.label || id;
  const isVolume = selected.value.kind === "volume";
  const hasBody =
    selected.value.kind === "chapter"
      ? await chapterHasGeneratedBody(id, selected.value.word_count ?? 0)
      : false;
  pendingCardDelete.value = { id, title, hasBody, isVolume, step: 1 };
}

function cancelPendingCardDelete() {
  if (deletingCard.value) return;
  pendingCardDelete.value = null;
}

async function confirmPendingCardDelete() {
  const p = pendingCardDelete.value;
  if (!p || !tree.value || deletingCard.value) return;
  // 有正文的章节 / 分卷：第二步再确认
  if ((p.hasBody || p.isVolume) && p.step === 1) {
    pendingCardDelete.value = { ...p, step: 2 };
    return;
  }

  deletingCard.value = true;
  const id = p.id;
  const axiomHost =
    tree.value && selected.value && isAxiomSlot(knowledgeSlot(selected.value))
      ? axiomHostCoreLawsId(tree.value, id)
      : tree.value
        ? axiomHostCoreLawsId(tree.value, id)
        : null;
  const locationHost =
    tree.value && selected.value && isLocationSlot(knowledgeSlot(selected.value))
      ? locationHostSpatiotemporalId(tree.value, id)
      : tree.value
        ? locationHostSpatiotemporalId(tree.value, id)
        : null;
  const socialHost =
    tree.value && selected.value && isSocialPowerChildSlot(knowledgeSlot(selected.value))
      ? socialPowerHostId(tree.value, id)
      : tree.value
        ? socialPowerHostId(tree.value, id)
        : null;
  try {
    tree.value = await api.deleteTreeCard(props.id, id);
    pruneTreeRefs(tree.value);
    if (axiomHost) {
      syncCoreLawsExtracted(tree.value, axiomHost);
      applyAutoLayout(tree.value.nodes, tree.value.edges);
      tree.value = await persistTree(tree.value);
    } else if (locationHost) {
      syncSpatiotemporalExtracted(tree.value, locationHost);
      applyAutoLayout(tree.value.nodes, tree.value.edges);
      tree.value = await persistTree(tree.value);
    } else if (socialHost) {
      syncSocialPowerExtracted(tree.value, socialHost);
      applyAutoLayout(tree.value.nodes, tree.value.edges);
      tree.value = await persistTree(tree.value);
    }
    removeNodes([id], true);
    syncFlowFromTree();

    if (refineBeforeByNode.value[id] !== undefined) {
      const next = { ...refineBeforeByNode.value };
      delete next[id];
      refineBeforeByNode.value = next;
    }

    pendingCardDelete.value = null;
    const prefer =
      (axiomHost && tree.value.nodes.find((n) => n.id === axiomHost)) ||
      (locationHost && tree.value.nodes.find((n) => n.id === locationHost)) ||
      (socialHost && tree.value.nodes.find((n) => n.id === socialHost)) ||
      tree.value.nodes.find((n) => n.kind === "novel");
    if (prefer) await selectNode(prefer);
    else {
      selected.value = null;
      void api.setWorkspaceSelection(null, null);
    }
    notice.value = t("workspace.cardDeleted");
  } catch (e) {
    notice.value = String(e);
  } finally {
    deletingCard.value = false;
  }
}

/** Delete/Backspace：根节点忽略；其他节点走落盘删除。 */
function onCanvasKeydown(ev: KeyboardEvent) {
  if (ev.key !== "Delete" && ev.key !== "Backspace") return;
  const el = ev.target as HTMLElement | null;
  if (el) {
    const tag = el.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable) return;
  }
  // 根节点：明确不响应
  if (!selected.value || selected.value.kind === "novel") {
    ev.preventDefault();
    ev.stopPropagation();
    return;
  }
  if (!canDeleteSelectedCard.value || pendingCardDelete.value) return;
  ev.preventDefault();
  ev.stopPropagation();
  void deleteSelectedCard();
}

const knowledgeDraft = computed(() => selected.value?.knowledge ?? emptyKnowledge());

function ensureKnowledgePayload() {
  if (!selected.value || selected.value.kind !== "knowledge") return null;
  if (!selected.value.knowledge) selected.value.knowledge = emptyKnowledge();
  return selected.value.knowledge;
}

async function persistSelectedKnowledge() {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const slot = knowledgeSlot(selected.value);
  if (isWorldviewFanSlot(slot)) applyWorldviewFanLabel(selected.value, slot);
  const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
  if (n) {
    n.knowledge = selected.value.knowledge;
    n.label = selected.value.label;
    // Tree preview mirrors extracted features.
    const feat = selected.value.knowledge?.extracted?.trim() ?? "";
    n.outline = feat ? [...feat].slice(0, 200).join("") : "";
  }
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
}

function applyWorldviewFanLabel(node: TreeNode, slot: string) {
  const title = worldviewFanTitle(t, slot);
  if (title) node.label = title;
}

async function persistSelectedCoreLaws(payload: {
  coreLaws: CoreLawsData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_core_laws");
  payload.coreLaws.axioms = [];
  k.core_laws = payload.coreLaws;
  k.extracted = payload.extracted;
  k.slot = "wv_core_laws";
  await persistSelectedKnowledge();
}

async function persistSelectedWorldAxiom(payload: {
  name: string;
  worldAxiom: WorldAxiom;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.world_axiom = normalizeAxiom(payload.worldAxiom);
  k.extracted = payload.extracted;
  k.slot = "wv_axiom";
  const hostId = axiomHostCoreLawsId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncCoreLawsExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function addCoreLawsAxiom() {
  if (!tree.value || !selected.value || !selectedIsCoreLaws.value) return;
  const n = linkedAxiomCards(tree.value, selected.value.id).length + 1;
  const node = createAxiomCard(
    tree.value,
    selected.value.id,
    t("workspace.coreLaws.axiomN", { n }),
  );
  if (!node) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function openCoreLawsAxiom(id: string) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function persistSelectedSpatiotemporal(payload: {
  spatiotemporal: SpatiotemporalData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_spatiotemporal");
  payload.spatiotemporal.locations = [];
  k.spatiotemporal = payload.spatiotemporal;
  k.extracted = payload.extracted;
  k.slot = "wv_spatiotemporal";
  await persistSelectedKnowledge();
}

async function persistSelectedExistence(payload: {
  existence: ExistenceData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_existence");
  k.existence = payload.existence;
  k.extracted = payload.extracted;
  k.slot = "wv_existence";
  await persistSelectedKnowledge();
}

async function persistSelectedInfoFlow(payload: {
  infoFlow: InfoFlowData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_info_flow");
  k.info_flow = payload.infoFlow;
  k.extracted = payload.extracted;
  k.slot = "wv_info_flow";
  await persistSelectedKnowledge();
}

async function persistSelectedKeyLocation(payload: {
  name: string;
  keyLocation: KeyLocation;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.key_location = normalizeLocation(payload.keyLocation);
  k.extracted = payload.extracted;
  k.slot = "wv_location";
  const hostId = locationHostSpatiotemporalId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncSpatiotemporalExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function addSpatiotemporalLocation() {
  if (!tree.value || !selected.value || !selectedIsSpatiotemporal.value) return;
  const n = linkedLocationCards(tree.value, selected.value.id).length + 1;
  const node = createLocationCard(
    tree.value,
    selected.value.id,
    t("workspace.st.locationN", { n }),
  );
  if (!node) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function openSpatiotemporalLocation(id: string) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function addSpatiotemporalLocations(locs: KeyLocation[]) {
  if (!tree.value || !selected.value || !selectedIsSpatiotemporal.value || !locs.length) return;
  let last: TreeNode | null = null;
  for (const loc of locs) {
    last = createLocationCard(
      tree.value,
      selected.value.id,
      loc.name.trim() || t("workspace.st.locationUntitled"),
      loc,
    );
  }
  if (!last) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function persistSelectedSocialPower(payload: {
  socialPower: SocialPowerData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_social_power");
  payload.socialPower.races = [];
  payload.socialPower.factions = [];
  k.social_power = payload.socialPower;
  k.extracted = payload.extracted;
  k.slot = "wv_social_power";
  await persistSelectedKnowledge();
}

async function persistSelectedWorldRace(payload: {
  name: string;
  worldRace: WorldRace;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.world_race = normalizeRace(payload.worldRace);
  k.extracted = payload.extracted;
  k.slot = "wv_race";
  const hostId = socialPowerHostId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncSocialPowerExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function persistSelectedMajorFaction(payload: {
  name: string;
  majorFaction: MajorFaction;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.major_faction = normalizeFaction(payload.majorFaction);
  k.extracted = payload.extracted;
  k.slot = "wv_faction";
  const hostId = socialPowerHostId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncSocialPowerExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function addSocialPowerRace() {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return;
  const n = linkedRaceCards(tree.value, selected.value.id).length + 1;
  const node = createRaceCard(
    tree.value,
    selected.value.id,
    t("workspace.pow.raceN", { n }),
  );
  if (!node) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function addSocialPowerFaction() {
  if (!tree.value || !selected.value || !selectedIsSocialPower.value) return;
  const n = linkedFactionCards(tree.value, selected.value.id).length + 1;
  const node = createFactionCard(
    tree.value,
    selected.value.id,
    t("workspace.pow.factionN", { n }),
  );
  if (!node) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function openSocialPowerRace(id: string) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function openSocialPowerFaction(id: string) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function publishSelectedKnowledge() {
  if (!selected.value || selected.value.kind !== "knowledge" || publishBusy.value) return;
  await persistSelectedKnowledge();
  const title = selected.value.label.trim() || t("workspace.newKnowledge");
  const k = selected.value.knowledge ?? emptyKnowledge();
  const draft = {
    title,
    book_ids: k.book_ids ?? [],
    extract_prompt: k.extract_prompt ?? "",
    extracted: k.extracted ?? "",
  };
  try {
    publicKnowledgeCards.value = await api.listPublicKnowledgeCards();
  } catch {
    /* list optional */
  }
  const same = publicKnowledgeCards.value.filter((c) => c.title.trim() === title);
  const dup = same.find((c) => !c.archived) ?? same[0];
  if (dup) {
    pendingPublishDup.value = { draft, existingId: dup.id };
    return;
  }
  await commitPublish(null, draft);
}

type PublishDraft = {
  title: string;
  book_ids: string[];
  extract_prompt: string;
  extracted: string;
};

const pendingPublishDup = ref<{ draft: PublishDraft; existingId: string } | null>(null);

async function commitPublish(id: string | null, draft: PublishDraft) {
  if (publishBusy.value) return;
  publishBusy.value = true;
  try {
    const card = await api.upsertPublicKnowledgeCard({
      id,
      title: draft.title,
      book_ids: draft.book_ids,
      extract_prompt: draft.extract_prompt,
      extracted: draft.extracted,
    });
    pendingPublishDup.value = null;
    publicKnowledgeCards.value = await api.listPublicKnowledgeCards();
    notice.value = t("workspace.publicKnowledgeSaved", { title: card.title });
  } catch (e) {
    notice.value = String(e);
  } finally {
    publishBusy.value = false;
  }
}

function cancelPublishDup() {
  if (publishBusy.value) return;
  pendingPublishDup.value = null;
}

const filteredPublicCards = computed(() => {
  const live = publicKnowledgeCards.value.filter((c) => !c.archived);
  const q = publicPickFilter.value.trim().toLowerCase();
  if (!q) return live;
  return live.filter((c) => c.title.toLowerCase().includes(q));
});

async function openPublicPick() {
  publicPickFilter.value = "";
  publicKnowledgeCards.value = await api.listPublicKnowledgeCards().catch(() => []);
  publicPickOpen.value = true;
}

async function addPickedPublicCard(id: string) {
  if (publicPickBusy.value) return;
  publicPickBusy.value = true;
  try {
    const before = new Set((tree.value?.nodes ?? []).map((n) => n.id));
    tree.value = await api.addPublicKnowledgeCard(props.id, id, selected.value?.id ?? null);
    pruneTreeRefs(tree.value);
    syncFlowFromTree();
    publicPickOpen.value = false;
    const added = tree.value.nodes.find((n) => n.kind === "knowledge" && !before.has(n.id));
    if (added) await selectNode(added);
    notice.value = t("workspace.publicKnowledgeAdded");
  } catch (e) {
    notice.value = String(e);
  } finally {
    publicPickBusy.value = false;
  }
}

const characterSaveBusy = ref(false);

async function persistSelectedCharacter(payload: {
  name: string;
  card: NonNullable<TreeNode["character"]>;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "character") return;
  characterSaveBusy.value = true;
  try {
    selected.value.label = payload.name;
    selected.value.character = { ...payload.card };
    const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
    if (n) {
      n.label = payload.name;
      n.character = { ...payload.card };
    }
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
    notice.value = t("character.saved");
  } catch (e) {
    notice.value = String(e);
  } finally {
    characterSaveBusy.value = false;
  }
}

async function persistSelectedCardText() {
  const sel = selected.value;
  if (!tree.value || !sel) return;
  if (
    sel.kind !== "novel" &&
    sel.kind !== "volume" &&
    sel.kind !== "chapter" &&
    sel.kind !== "side_plot"
  )
    return;
  const label = sel.label.trim();
  if ((sel.kind === "chapter" || sel.kind === "side_plot" || sel.kind === "volume") && !label)
    return;
  const n = tree.value.nodes.find((x) => x.id === sel.id);
  if (!n) return;
  if (sel.kind === "chapter" || sel.kind === "side_plot" || sel.kind === "volume") {
    sel.label = label;
    n.label = label;
  }
  n.outline = sel.outline;
  if (sel.kind === "side_plot") {
    n.side_plot = sel.side_plot
      ? { ...sel.side_plot }
      : { status: "active", absorbed: false };
  }
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
}

function plotStatusOf(n: TreeNode | null | undefined): string {
  if (!n || n.kind !== "side_plot") return "active";
  const s = (n.side_plot?.status || "active").trim().toLowerCase();
  if (s === "resolved" || s === "deferred") return s;
  return "active";
}

async function setSelectedPlotStatus(status: "active" | "resolved" | "deferred") {
  const sel = selected.value;
  if (!sel || sel.kind !== "side_plot") return;
  const meta = { ...(sel.side_plot ?? { status: "active", absorbed: false }), status };
  sel.side_plot = meta;
  await persistSelectedCardText();
}

async function toggleSelectedPlotAbsorbed() {
  const sel = selected.value;
  if (!sel || sel.kind !== "side_plot") return;
  const cur = sel.side_plot ?? { status: "active", absorbed: false };
  sel.side_plot = { ...cur, absorbed: !cur.absorbed };
  await persistSelectedCardText();
}

async function persistNovelPlan() {
  if (!novel.value) return;
  try {
    novel.value = await api.updateNovelPlan(
      props.id,
      Number(novel.value.word_count_min) || 2000,
      Number(novel.value.word_count_max) || 3000,
      Number(novel.value.chapter_count) || 20,
    );
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value?.kind === "novel") {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
  } catch (e) {
    notice.value = String(e);
  }
}

const hasRefineDiff = computed(() => {
  const id = selected.value?.id;
  return !!(id && refineBeforeByNode.value[id] != null && chapterMd.value);
});

const refineDiffHunks = computed(() => {
  const id = selected.value?.id;
  if (!id) return [];
  const before = refineBeforeByNode.value[id];
  if (before == null) return [];
  return diffLines(before, chapterMd.value);
});


/** 本章继承的根剧情（只读） */
const chapterLinkedPlotsFromRoot = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) return [] as { id: string; label: string }[];
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  return rootLinkedPlotIds(tr.nodes, tr.edges).map((id) => ({
    id,
    label: byId.get(id)?.label ?? id,
  }));
});

/** 本章继承的分卷剧情（只读；= 继承并集 − 根） */
const chapterLinkedPlotsFromVolume = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) return [] as { id: string; label: string }[];
  const rootSet = new Set(rootLinkedPlotIds(tr.nodes, tr.edges));
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  return chapterInheritedPlotIds(ch.id, tr.nodes, tr.edges)
    .filter((id) => !rootSet.has(id))
    .map((id) => ({
      id,
      label: byId.get(id)?.label ?? id,
    }));
});

/** 本章关联剧情（仅本章；linked_side_plot_ids 顺序，0 = 最上） */
const chapterLinkedPlotsLocal = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) return [] as { id: string; label: string; order: number }[];
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  return chapterLocalPlotIds(ch.id, tr.nodes, tr.edges).map((id, order) => ({
    id,
    order,
    label: byId.get(id)?.label ?? id,
  }));
});

/** 分卷：根继承剧情（只读） */
const volumeLinkedPlotsFromRoot = computed(() => {
  const vol = selected.value;
  const tr = tree.value;
  if (!vol || vol.kind !== "volume" || !tr) return [] as { id: string; label: string }[];
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  return rootLinkedPlotIds(tr.nodes, tr.edges).map((id) => ({
    id,
    label: byId.get(id)?.label ?? id,
  }));
});

/** 分卷本机剧情（可排序） */
const volumeLinkedPlotsLocal = computed(() => {
  const vol = selected.value;
  const tr = tree.value;
  if (!vol || vol.kind !== "volume" || !tr) return [] as { id: string; label: string; order: number }[];
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  return volumeLocalPlotIds(vol.id, tr.nodes, tr.edges).map((id, order) => ({
    id,
    order,
    label: byId.get(id)?.label ?? id,
  }));
});

const plotListEl = ref<HTMLElement | null>(null);
const plotReorderDragFrom = ref<number | null>(null);
const plotReorderOver = ref<number | null>(null);

function applyHostPlotOrder(from: number, to: number) {
  const sel = selected.value;
  if (!sel || (sel.kind !== "chapter" && sel.kind !== "volume")) return;
  if (from === to || from < 0 || to < 0) return;
  const ids =
    sel.kind === "chapter"
      ? chapterLinkedPlotsLocal.value.map((p) => p.id)
      : volumeLinkedPlotsLocal.value.map((p) => p.id);
  if (from >= ids.length || to >= ids.length) return;
  const [item] = ids.splice(from, 1);
  ids.splice(to, 0, item);
  void reorderHostPlots(sel.id, sel.kind, ids);
}

/** 按指针 Y 落到哪一行（中线以上算该行） */
function plotIndexAtClientY(clientY: number): number | null {
  const root = plotListEl.value;
  if (!root) return null;
  const items = [...root.querySelectorAll<HTMLElement>("[data-plot-idx]")];
  if (!items.length) return null;
  for (const el of items) {
    const r = el.getBoundingClientRect();
    if (clientY < r.top + r.height / 2) {
      const n = Number(el.dataset.plotIdx);
      return Number.isFinite(n) ? n : null;
    }
  }
  const last = Number(items[items.length - 1]?.dataset.plotIdx);
  return Number.isFinite(last) ? last : null;
}

/** ponytail: Tauri WKWebView 的 HTML5 drop 经常不触发；改 pointer 排序 */
function onHostPlotPointerDown(index: number, ev: PointerEvent) {
  if (ev.button !== 0) return;
  ev.preventDefault();
  plotReorderDragFrom.value = index;
  plotReorderOver.value = index;
  const onMove = (e: PointerEvent) => {
    const over = plotIndexAtClientY(e.clientY);
    if (over !== null) plotReorderOver.value = over;
  };
  const onUp = (e: PointerEvent) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    const from = plotReorderDragFrom.value;
    const to = plotIndexAtClientY(e.clientY) ?? plotReorderOver.value;
    plotReorderDragFrom.value = null;
    plotReorderOver.value = null;
    if (from === null || to === null) return;
    applyHostPlotOrder(from, to);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
}

/** 本章关联知识卡（写作手法/文风等硬约束；linked_* ∪ 边） */
const chapterLinkedKnowledge = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) {
    return [] as {
      id: string;
      label: string;
      snippet: string;
      from: "root" | "volume" | "chapter";
      chipShell: string;
    }[];
  }
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  const rootSet = new Set(rootLinkedKnowledgeIds(tr.nodes, tr.edges));
  const vid = chapterParentVolumeId(ch.id, tr.nodes, tr.edges);
  const volSet = new Set(
    vid
      ? chapterInheritedKnowledgeIds(ch.id, tr.nodes, tr.edges).filter((id) => !rootSet.has(id))
      : [],
  );
  return sortLinkedKnowledgeNodes(
    filterHostPanelLinkedKnowledge(
      chapterEffectiveKnowledgeIds(ch.id, tr.nodes, tr.edges)
        .map((id) => byId.get(id))
        .filter((n): n is NonNullable<typeof n> => !!n && n.kind === "knowledge"),
    ),
  ).map((n) => {
      const kn = n.knowledge;
      const snippet =
        (kn?.extracted || kn?.extract_prompt || n.outline || "").trim().slice(0, 120);
      const from: "root" | "volume" | "chapter" = rootSet.has(n.id)
        ? "root"
        : volSet.has(n.id)
          ? "volume"
          : "chapter";
      return {
        id: n.id,
        label: knowledgeNodeLabel(n, t),
        snippet,
        from,
        chipShell: knowledgeChipShell(n),
      };
    });
});

const volumeLinkedKnowledge = computed(() => {
  const vol = selected.value;
  const tr = tree.value;
  if (!vol || vol.kind !== "volume" || !tr) {
    return [] as {
      id: string;
      label: string;
      snippet: string;
      from: "root" | "volume";
      chipShell: string;
    }[];
  }
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  const rootSet = new Set(rootLinkedKnowledgeIds(tr.nodes, tr.edges));
  return sortLinkedKnowledgeNodes(
    filterHostPanelLinkedKnowledge(
      volumeEffectiveKnowledgeIds(vol.id, tr.nodes, tr.edges)
        .map((id) => byId.get(id))
        .filter((n): n is NonNullable<typeof n> => !!n && n.kind === "knowledge"),
    ),
  ).map((n) => {
      const kn = n.knowledge;
      const snippet =
        (kn?.extracted || kn?.extract_prompt || n.outline || "").trim().slice(0, 120);
      return {
        id: n.id,
        label: knowledgeNodeLabel(n, t),
        snippet,
        from: (rootSet.has(n.id) ? "root" : "volume") as "root" | "volume",
        chipShell: knowledgeChipShell(n),
      };
    });
});

/** 章/卷关联人物名：含根（章还含父卷）贯穿人物、剧情卡上挂的人物 */
function collectHostLinkedCharLabels(
  host: { id: string; kind: string; linked_character_ids: string[] },
  tr: { nodes: TreeNode[]; edges: TreeEdge[] },
): string[] {
  const charIds = new Set<string>(host.linked_character_ids ?? []);
  const plotIds = new Set<string>();

  if (host.kind === "chapter") {
    for (const id of chapterLocalPlotIds(host.id, tr.nodes, tr.edges)) plotIds.add(id);
    for (const id of chapterInheritedPlotIds(host.id, tr.nodes, tr.edges)) plotIds.add(id);
  } else if (host.kind === "volume") {
    for (const id of rootLinkedPlotIds(tr.nodes, tr.edges)) plotIds.add(id);
    for (const id of volumeLocalPlotIds(host.id, tr.nodes, tr.edges)) plotIds.add(id);
  }

  for (const e of tr.edges) {
    if (e.source !== host.id && e.target !== host.id) continue;
    const otherId = e.source === host.id ? e.target : e.source;
    const other = tr.nodes.find((n) => n.id === otherId);
    if (!other) continue;
    if (other.kind === "character") charIds.add(otherId);
    if (other.kind === "side_plot") plotIds.add(otherId);
  }

  const root = tr.nodes.find((n) => n.kind === "novel");
  if (root) {
    for (const cid of root.linked_character_ids ?? []) charIds.add(cid);
    for (const e of tr.edges) {
      if (e.source !== root.id && e.target !== root.id) continue;
      const otherId = e.source === root.id ? e.target : e.source;
      if (tr.nodes.find((n) => n.id === otherId)?.kind === "character") charIds.add(otherId);
    }
  }

  if (host.kind === "chapter") {
    const vid = chapterParentVolumeId(host.id, tr.nodes, tr.edges);
    if (vid) {
      const vol = tr.nodes.find((n) => n.id === vid);
      if (vol) for (const cid of vol.linked_character_ids ?? []) charIds.add(cid);
      for (const e of tr.edges) {
        if (e.source !== vid && e.target !== vid) continue;
        const otherId = e.source === vid ? e.target : e.source;
        if (tr.nodes.find((n) => n.id === otherId)?.kind === "character") charIds.add(otherId);
      }
    }
  }

  for (const pid of [...plotIds]) {
    const plot = tr.nodes.find((n) => n.id === pid);
    if (plot) for (const cid of plot.linked_character_ids ?? []) charIds.add(cid);
    for (const e of tr.edges) {
      if (e.source !== pid && e.target !== pid) continue;
      const otherId = e.source === pid ? e.target : e.source;
      if (tr.nodes.find((n) => n.id === otherId)?.kind === "character") charIds.add(otherId);
    }
  }

  return [...charIds]
    .map((id) => tr.nodes.find((n) => n.id === id)?.label)
    .filter((x): x is string => !!x);
}

const chapterLinkTags = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) return { chars: [] as string[] };
  return { chars: collectHostLinkedCharLabels(ch, tr) };
});

const volumeLinkTags = computed(() => {
  const vol = selected.value;
  const tr = tree.value;
  if (!vol || vol.kind !== "volume" || !tr) return { chars: [] as string[] };
  return { chars: collectHostLinkedCharLabels(vol, tr) };
});

const chapterBusy = computed(
  () =>
    busy.value === "generate" ||
    busy.value === "refine" ||
    busy.value === "plan-next" ||
    busy.value === "gen-plots" ||
    busy.value === "regen-memory" ||
    busy.value === "gen-cards" ||
    busy.value === "consolidate-plots",
);
/** 三区尺寸比例跨会话记住 */
const leftW = useLocalStorage("novework.workspaceLeftW", 380);
const rightW = useLocalStorage("novework.workspaceRightW", 320);
/** 正文面板停靠：右侧栏 | 树图下方 */
const chatDock = useLocalStorage<"right" | "bottom">("novework.chatDock", "right");
const chatBottomH = useLocalStorage("novework.chatBottomH", 280);
const MIN_SIDE = 220;
const MIN_MID = 280;
const MIN_CHAT_H = 160;

const workspaceGridStyle = computed(() => {
  // 左栏（章节编辑）始终满高独立；树图与正文面板只在右侧区域切换
  if (chatDock.value === "bottom") {
    return {
      display: "grid",
      height: "100%",
      gridTemplateColumns: `${leftW.value}px 6px minmax(${MIN_MID}px, 1fr)`,
      gridTemplateRows: `minmax(0, 1fr) 6px ${chatBottomH.value}px`,
      gridTemplateAreas: `"left v1 mid" "left v1 hr" "left v1 chat"`,
    };
  }
  return {
    display: "grid",
    height: "100%",
    gridTemplateColumns: `${leftW.value}px 6px minmax(${MIN_MID}px, 1fr) 6px ${rightW.value}px`,
    gridTemplateRows: "minmax(0, 1fr)",
    gridTemplateAreas: `"left v1 mid v2 chat"`,
  };
});

function startResize(which: "left" | "right", ev: MouseEvent) {
  ev.preventDefault();
  const startX = ev.clientX;
  const startLeft = leftW.value;
  const startRight = rightW.value;
  const onMove = (e: MouseEvent) => {
    const dx = e.clientX - startX;
    const total = window.innerWidth;
    if (which === "left") {
      const max =
        chatDock.value === "bottom"
          ? total - MIN_MID - 16
          : total - rightW.value - MIN_MID - 16;
      leftW.value = Math.min(Math.max(startLeft + dx, MIN_SIDE), max);
    } else {
      rightW.value = Math.min(
        Math.max(startRight - dx, MIN_SIDE),
        total - leftW.value - MIN_MID - 16,
      );
    }
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function startResizeChatH(ev: MouseEvent) {
  ev.preventDefault();
  const startY = ev.clientY;
  const startH = chatBottomH.value;
  const onMove = (e: MouseEvent) => {
    const dy = startY - e.clientY;
    const max = Math.max(MIN_CHAT_H, Math.floor(window.innerHeight * 0.7));
    chatBottomH.value = Math.min(Math.max(startH + dy, MIN_CHAT_H), max);
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

async function onNodeDragStop(ev: { node: { id: string; position: { x: number; y: number } } }) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === ev.node.id);
  if (!n) return;
  n.position = { x: ev.node.position.x, y: ev.node.position.y };
  tree.value = await persistTree(tree.value);
}
</script>

<template>
  <div class="flex h-full flex-col">
    <div class="min-h-0 flex-1" :style="workspaceGridStyle">
      <!-- 左：卡片属性 -->
      <div class="flex min-h-0 min-w-0 flex-col border-r" style="grid-area: left">
        <div class="shrink-0 space-y-2 border-b p-3">
          <div class="flex items-start justify-between gap-2">
            <div
              v-if="selectedIsChapter || selected?.kind === 'side_plot' || selectedIsVolume"
              class="min-w-0 flex-1 space-y-1"
            >
              <label class="block text-[11px] text-muted-foreground">{{
                selectedIsChapter
                  ? t("workspace.chapterTitle")
                  : selectedIsVolume
                    ? t("workspace.volumeTitle")
                    : t("workspace.plotTitle")
              }}</label>
              <Input
                class="h-8 text-sm font-medium"
                :model-value="selected?.label ?? ''"
                :placeholder="
                  selectedIsChapter
                    ? t('workspace.chapterTitlePh')
                    : selectedIsVolume
                      ? t('workspace.volumeTitlePh')
                      : t('workspace.plotTitlePh')
                "
                :disabled="chapterBusy"
                @update:model-value="(v) => { if (selected) selected.label = String(v); }"
                @change="persistSelectedCardText"
              />
            </div>
            <div v-else class="min-w-0 flex-1 text-sm font-medium">
              {{ selected?.label ?? t("workspace.selectNode") }}
            </div>
            <Button
              v-if="canDeleteSelectedCard"
              size="sm"
              variant="destructive"
              class="h-7 shrink-0 text-xs"
              @click="deleteSelectedCard"
            >
              {{ t("workspace.deleteCard") }}
            </Button>
          </div>
          <div
            v-if="selected?.kind === 'side_plot'"
            class="space-y-1.5"
          >
            <label class="block text-[11px] text-muted-foreground">{{
              t("workspace.plotStatus")
            }}</label>
            <div class="flex flex-wrap gap-1">
              <button
                v-for="st in (['active', 'resolved', 'deferred'] as const)"
                :key="st"
                type="button"
                class="rounded px-2 py-1 text-[11px] transition-colors"
                :class="
                  plotStatusOf(selected) === st
                    ? 'bg-sky-600 text-white'
                    : 'bg-muted text-muted-foreground hover:bg-muted/80'
                "
                :disabled="chapterBusy"
                @click="setSelectedPlotStatus(st)"
              >
                {{
                  st === "active"
                    ? t("workspace.plotStatusActive")
                    : st === "resolved"
                      ? t("workspace.plotStatusResolved")
                      : t("workspace.plotStatusDeferred")
                }}
              </button>
            </div>
            <button
              type="button"
              class="text-[11px] underline-offset-2 hover:underline"
              :class="
                selected.side_plot?.absorbed
                  ? 'text-amber-700'
                  : 'text-muted-foreground'
              "
              :disabled="chapterBusy"
              @click="toggleSelectedPlotAbsorbed"
            >
              {{
                selected.side_plot?.absorbed
                  ? t("workspace.plotAbsorbedOn")
                  : t("workspace.plotAbsorbedOff")
              }}
            </button>
            <p class="text-[10px] text-muted-foreground">{{ t("workspace.plotStatusHint") }}</p>
          </div>
          <p v-if="chapterBusy" class="space-y-1 text-xs text-primary">
            <span class="flex items-center gap-1.5">
              <Loader2 class="h-3.5 w-3.5 shrink-0 animate-spin" />
              {{ chapterProgressLabel || t("workspace.waitingResult") }}
            </span>
            <span
              v-if="chapterProgress"
              class="block text-[11px] text-muted-foreground"
            >
              {{ t("workspace.taskProgress", { i: chapterProgress.index, n: chapterProgress.total }) }}
              · {{ formatStepMs(chapterTotalMs) }}
              <template v-if="chapterTokens">
                ·
                {{
                  t(
                    chapterTokens.confirmed
                      ? "workspace.taskPromptTokens"
                      : "workspace.taskPromptTokensEst",
                    { n: chapterTokens.prompt.toLocaleString() },
                  )
                }}
              </template>
            </span>
          </p>
          <p v-else-if="notice" class="text-xs text-muted-foreground">{{ notice }}</p>
        </div>
        <div
          class="relative min-h-0 flex-1 p-4"
          :class="
            selectedIsChapter || selected?.kind === 'side_plot' || selectedIsNovel || selectedIsVolume
              ? 'flex flex-col overflow-hidden'
              : 'overflow-auto'
          "
        >
          <div
            v-if="chapterBusy"
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-background/70 px-6 backdrop-blur-[1px]"
          >
            <Loader2 class="h-6 w-6 animate-spin text-primary" />
            <p class="text-sm font-medium text-foreground">
              {{
                busy === "refine"
                  ? t("workspace.refining")
                  : busy === "plan-next"
                    ? t("workspace.planningNext")
                    : busy === "gen-plots"
                      ? t("workspace.genPlotsBusy")
                      : busy === "regen-memory"
                        ? t("workspace.regenMemoryBusy")
                        : busy === "gen-cards"
                          ? t("workspace.rootGenChaptersBusy")
                          : t("workspace.generating")
              }}
            </p>
            <div v-if="chapterProgress" class="w-full max-w-sm space-y-2">
              <div class="flex justify-between text-[11px] text-muted-foreground">
                <span>{{ t("workspace.taskProgress", { i: chapterProgress.index, n: chapterProgress.total }) }}</span>
                <span>{{ chapterProgressPct }}%</span>
              </div>
              <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  class="h-full rounded-full bg-primary transition-[width] duration-300"
                  :style="{ width: chapterProgressPct + '%' }"
                />
              </div>
              <ul class="max-h-48 space-y-1 overflow-y-auto rounded-md border bg-background/90 px-3 py-2 text-left">
                <li
                  v-for="(s, i) in chapterStepTimingsView"
                  :key="i"
                  class="flex items-start justify-between gap-3 text-[11px] leading-snug"
                >
                  <span :class="s.done ? 'text-muted-foreground' : 'font-medium text-foreground'">
                    <span v-if="!s.done" class="mr-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-primary align-middle" />
                    {{ s.label }}
                  </span>
                  <span class="shrink-0 tabular-nums text-muted-foreground">{{ formatStepMs(s.ms) }}</span>
                </li>
              </ul>
              <p class="text-center text-[11px] text-muted-foreground">
                {{ t("workspace.taskTotalTime", { t: formatStepMs(chapterTotalMs) }) }}
              </p>
              <p
                v-if="chapterTokens"
                class="text-center text-[11px] tabular-nums text-muted-foreground"
              >
                {{
                  t(
                    chapterTokens.confirmed
                      ? "workspace.taskPromptTokens"
                      : "workspace.taskPromptTokensEst",
                    { n: chapterTokens.prompt.toLocaleString() },
                  )
                }}
                <template v-if="chapterTokens.confirmed && chapterTokens.completion > 0">
                  ·
                  {{
                    t("workspace.taskCompletionTokens", {
                      n: chapterTokens.completion.toLocaleString(),
                    })
                  }}
                </template>
              </p>
            </div>
            <Button size="sm" variant="destructive" @click="stopChapter">
              <Square class="mr-1.5 h-3.5 w-3.5" />
              {{ t("workspace.stop") }}
            </Button>
          </div>
          <CharacterCardPanel
            v-if="selectedIsCharacter"
            :name="selected?.label ?? ''"
            :card="selected?.character ?? emptyCharacter()"
            :busy="characterSaveBusy"
            @save="persistSelectedCharacter"
          />
          <CoreLawsPanel
            v-else-if="selectedIsCoreLaws"
            :novel-id="id"
            :data="selected?.knowledge?.core_laws"
            :axiom-links="coreLawsAxiomLinks"
            :linked-axioms="coreLawsLinkedAxioms"
            @save="persistSelectedCoreLaws"
            @add-axiom="addCoreLawsAxiom"
            @open-axiom="openCoreLawsAxiom"
          />
          <WorldAxiomPanel
            v-else-if="selectedIsWorldAxiom"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.world_axiom"
            @save="persistSelectedWorldAxiom"
          />
          <SocialPowerPanel
            v-else-if="selectedIsSocialPower"
            :novel-id="id"
            :data="selected?.knowledge?.social_power"
            :race-links="socialPowerRaceLinks"
            :faction-links="socialPowerFactionLinks"
            :linked-races="socialPowerLinkedRaces"
            :linked-factions="socialPowerLinkedFactions"
            @save="persistSelectedSocialPower"
            @add-race="addSocialPowerRace"
            @add-faction="addSocialPowerFaction"
            @open-race="openSocialPowerRace"
            @open-faction="openSocialPowerFaction"
          />
          <WorldRacePanel
            v-else-if="selectedIsWorldRace"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.world_race"
            @save="persistSelectedWorldRace"
          />
          <MajorFactionPanel
            v-else-if="selectedIsMajorFaction"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.major_faction"
            @save="persistSelectedMajorFaction"
          />
          <KeyLocationPanel
            v-else-if="selectedIsKeyLocation"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.key_location"
            @save="persistSelectedKeyLocation"
          />
          <SpatiotemporalPanel
            v-else-if="selectedIsSpatiotemporal"
            :novel-id="id"
            :data="selected?.knowledge?.spatiotemporal"
            :location-links="spatiotemporalLocationLinks"
            :linked-locations="spatiotemporalLinkedLocations"
            @save="persistSelectedSpatiotemporal"
            @add-location="addSpatiotemporalLocation"
            @open-location="openSpatiotemporalLocation"
            @add-locations="addSpatiotemporalLocations"
          />
          <ExistencePanel
            v-else-if="selectedIsExistence"
            :novel-id="id"
            :data="selected?.knowledge?.existence"
            @save="persistSelectedExistence"
          />
          <InfoFlowPanel
            v-else-if="selectedIsInfoFlow"
            :novel-id="id"
            :data="selected?.knowledge?.info_flow"
            @save="persistSelectedInfoFlow"
          />
          <div v-else-if="selectedIsKnowledge" class="space-y-4 text-sm">
            <div>
              <div class="mb-1 flex items-start justify-between gap-2">
                <div class="min-w-0 flex-1">
                  <WorldviewFanTitle v-if="selectedWorldviewFanSlot" :slot="selectedWorldviewFanSlot" />
                  <label v-else class="mb-0 block text-xs text-muted-foreground">{{ t("workspace.knowledgeLabel") }}</label>
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  class="h-7 shrink-0 px-2 text-xs"
                  :disabled="publishBusy"
                  :title="t('workspace.publishKnowledge')"
                  @click="publishSelectedKnowledge"
                >
                  <Loader2 v-if="publishBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
                  <Share2 v-else class="mr-1 h-3.5 w-3.5" />
                  {{ t("workspace.publishKnowledge") }}
                </Button>
              </div>
              <Input
                v-if="!selectedWorldviewFanSlot"
                :model-value="selected?.label ?? ''"
                class="h-8"
                @update:model-value="(v) => { if (selected) selected.label = String(v); }"
                @change="persistSelectedKnowledge"
              />
            </div>
            <p class="text-[11px] text-muted-foreground">{{ t("workspace.knowledgeHint") }}</p>
            <div class="space-y-1">
              <label class="block text-xs text-muted-foreground">{{ t("workspace.knowledgeFeatures") }}</label>
              <Textarea
                :model-value="knowledgeDraft.extracted"
                rows="10"
                class="min-h-[10rem] text-sm"
                :placeholder="t('workspace.knowledgeFeaturesPh')"
                @update:model-value="(v) => { const k = ensureKnowledgePayload(); if (k) k.extracted = String(v); }"
                @change="persistSelectedKnowledge"
              />
              <p class="text-[11px] text-muted-foreground">
                {{
                  t("workspace.knowledgeFeaturesHint", {
                    n: (knowledgeDraft.extracted || "").length,
                    cap: 500,
                  })
                }}
              </p>
            </div>
          </div>
          <div v-else-if="selectedIsNovel" class="flex min-h-0 flex-1 flex-col gap-4">
            <div class="min-h-0 shrink overflow-y-auto space-y-4">
            <div class="space-y-2">
              <label class="block text-xs text-muted-foreground">{{ t("workspace.cover") }}</label>
              <div class="flex items-start gap-3">
                <div
                  class="flex h-36 w-24 shrink-0 items-center justify-center overflow-hidden rounded border bg-muted text-[10px] text-muted-foreground"
                >
                  <img
                    v-if="coverUrl"
                    :src="coverUrl"
                    class="h-full w-full object-cover"
                    alt=""
                  />
                  <span v-else>{{ t("workspace.cover") }}</span>
                </div>
                <div class="flex min-w-0 flex-1 flex-col gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    class="justify-start"
                    :disabled="coverBusy"
                    @click="pickAndSetCover"
                  >
                    <Loader2 v-if="coverBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                    {{ t("workspace.pickCover") }}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    class="justify-start"
                    :disabled="coverPromptBusy"
                    :title="t('workspace.coverPromptHint')"
                    @click="generateCoverPrompt"
                  >
                    <Loader2
                      v-if="coverPromptBusy"
                      class="mr-1.5 h-3.5 w-3.5 shrink-0 animate-spin"
                    />
                    <Sparkles v-else class="mr-1.5 h-3.5 w-3.5 shrink-0" />
                    {{
                      coverPromptBusy
                        ? t("workspace.genCoverPromptBusy")
                        : t("workspace.genCoverPrompt")
                    }}
                  </Button>
                </div>
              </div>
              <div v-if="coverPrompt || coverPromptBusy" class="space-y-1.5">
                <div class="flex items-center justify-between gap-2">
                  <label class="text-xs text-muted-foreground">{{ t("workspace.coverPrompt") }}</label>
                  <Button
                    v-if="coverPrompt"
                    variant="ghost"
                    size="sm"
                    class="h-7 px-2 text-xs"
                    @click="copyCoverPrompt"
                  >
                    <Copy class="mr-1 h-3 w-3" />
                    {{ coverPromptHint || t("workspace.coverPromptCopy") }}
                  </Button>
                </div>
                <Textarea
                  v-model="coverPrompt"
                  rows="4"
                  class="text-xs"
                  :placeholder="t('workspace.coverPromptPh')"
                />
              </div>
            </div>
            <div class="space-y-2 rounded-md border bg-muted/30 p-3">
              <div class="grid gap-3 sm:grid-cols-2">
                <div>
                  <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.planWords") }}</label>
                  <div class="flex items-center gap-1.5">
                    <Input
                      type="number"
                      min="1"
                      class="h-8"
                      :model-value="novel?.word_count_min ?? 2000"
                      @update:model-value="(v) => { if (novel) novel.word_count_min = Number(v) || 0; }"
                      @change="persistNovelPlan"
                    />
                    <span class="text-xs text-muted-foreground">–</span>
                    <Input
                      type="number"
                      min="1"
                      class="h-8"
                      :model-value="novel?.word_count_max ?? 3000"
                      @update:model-value="(v) => { if (novel) novel.word_count_max = Number(v) || 0; }"
                      @change="persistNovelPlan"
                    />
                  </div>
                </div>
                <div>
                  <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.planChapters") }}</label>
                  <Input
                    type="number"
                    min="1"
                    max="500"
                    class="h-8"
                    :model-value="novel?.chapter_count ?? 20"
                    @update:model-value="(v) => { if (novel) novel.chapter_count = Number(v) || 0; }"
                    @change="persistNovelPlan"
                  />
                </div>
              </div>
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.planHint") }}</p>
            </div>
            <div class="space-y-2 rounded-md border bg-muted/30 p-3">
              <div class="flex flex-wrap items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  class="justify-start"
                  :disabled="worldviewBusy"
                  @click="openWorldviewChat"
                >
                  <Loader2 v-if="worldviewBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  <Sparkles v-else class="mr-1.5 h-3.5 w-3.5" />
                  {{
                    worldviewBusy
                      ? t("workspace.wv.busy")
                      : rootWorldviewComplete
                        ? t("workspace.wv.chatOpen")
                        : t("workspace.wv.generate")
                  }}
                </Button>
              </div>
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.wv.hint") }}</p>
            </div>
            </div>
            <div class="flex min-h-[12rem] flex-1 flex-col space-y-2">
              <label class="block shrink-0 text-xs text-muted-foreground">{{ t("workspace.rootOutline") }}</label>
              <div class="relative min-h-0 flex-1">
                <Textarea
                  :model-value="selected?.outline ?? ''"
                  class="absolute inset-0 resize-none overflow-y-auto text-sm"
                  :placeholder="t('workspace.rootOutlinePh')"
                  @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                  @change="persistSelectedCardText"
                />
              </div>
              <p class="shrink-0 text-xs text-muted-foreground">{{ t("workspace.rootOutlineHint") }}</p>
            </div>
          </div>
          <div v-else-if="selectedIsVolume" class="flex min-h-0 flex-1 flex-col gap-2">
            <div class="min-h-0 shrink overflow-y-auto space-y-2">
              <div class="space-y-1.5 rounded-md border bg-violet-50/50 p-2 dark:bg-violet-950/20">
                <p class="text-[11px] font-medium text-violet-900 dark:text-violet-100">
                  {{ t("workspace.volumeLinkedPlots") }}
                </p>
                <p class="text-[10px] text-muted-foreground">{{ t("workspace.plotReorderHint") }}</p>
                <ul v-if="volumeLinkedPlotsFromRoot.length" class="mb-1.5 space-y-1">
                  <li
                    v-for="p in volumeLinkedPlotsFromRoot"
                    :key="'root-' + p.id"
                    class="flex items-center gap-2 rounded border border-dashed bg-muted/40 px-2 py-1.5 text-xs"
                  >
                    <span
                      class="shrink-0 rounded bg-muted px-1 py-0.5 text-[9px] font-medium text-muted-foreground"
                    >{{ t("workspace.chapterPlotInherited") }}</span>
                    <span class="min-w-0 flex-1 truncate">{{ p.label }}</span>
                  </li>
                </ul>
                <ul v-if="volumeLinkedPlotsLocal.length" ref="plotListEl" class="space-y-1">
                  <li
                    v-for="(p, i) in volumeLinkedPlotsLocal"
                    :key="p.id"
                    :data-plot-idx="i"
                    class="flex cursor-grab items-center gap-2 rounded border bg-background px-2 py-1.5 text-xs select-none active:cursor-grabbing"
                    :class="{
                      'opacity-40': plotReorderDragFrom === i,
                      'ring-2 ring-violet-500':
                        plotReorderDragFrom !== null &&
                        plotReorderOver === i &&
                        plotReorderDragFrom !== i,
                    }"
                    @pointerdown="onHostPlotPointerDown(i, $event)"
                  >
                    <span
                      class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-violet-500 text-[10px] font-medium text-white"
                    >{{ i }}</span>
                    <span class="min-w-0 flex-1 truncate">{{ p.label }}</span>
                  </li>
                </ul>
                <p
                  v-if="!volumeLinkedPlotsFromRoot.length && !volumeLinkedPlotsLocal.length"
                  class="text-[10px] text-muted-foreground"
                >{{ t("workspace.volumeLinkedPlotsEmpty") }}</p>
              </div>
              <div class="space-y-1.5 rounded-md border bg-teal-50/50 p-2 dark:bg-teal-950/20">
                <p class="text-[11px] font-medium text-teal-900 dark:text-teal-100">
                  {{ t("workspace.volumeLinkedKnowledge") }}
                </p>
                <ul v-if="volumeLinkedKnowledge.length" class="flex flex-wrap items-start justify-start gap-1">
                  <li
                    v-for="k in volumeLinkedKnowledge"
                    :key="k.id"
                    class="inline-flex max-w-full items-center gap-1.5 rounded border px-2 py-1 text-xs"
                    :class="k.chipShell"
                  >
                    <span
                      class="mt-0.5 shrink-0 rounded bg-muted px-1 py-0.5 text-[9px] font-medium text-muted-foreground"
                    >{{
                      k.from === "root"
                        ? t("workspace.chapterPlotInherited")
                        : t("workspace.volumePlotInherited")
                    }}</span>
                    <div class="min-w-0">
                      <span class="block max-w-[180px] truncate font-medium">{{ k.label }}</span>
                    </div>
                  </li>
                </ul>
                <p v-else class="text-[10px] text-muted-foreground">{{ t("workspace.volumeLinkedKnowledgeEmpty") }}</p>
              </div>
              <div
                v-if="volumeLinkTags.chars.length"
                class="flex flex-wrap gap-1"
              >
                <span
                  v-for="name in volumeLinkTags.chars"
                  :key="'vc-' + name"
                  class="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] text-amber-900"
                  :title="t('workspace.role')"
                >{{ name }}</span>
              </div>
            </div>
            <div class="flex min-h-[12rem] flex-1 flex-col space-y-2">
              <label class="block shrink-0 text-xs text-muted-foreground">{{ t("workspace.volumeOutline") }}</label>
              <div class="relative min-h-0 flex-1">
                <Textarea
                  :model-value="selected?.outline ?? ''"
                  class="absolute inset-0 resize-none overflow-y-auto text-sm"
                  :placeholder="t('workspace.volumeOutlinePh')"
                  @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                  @change="persistSelectedCardText"
                />
              </div>
              <p class="shrink-0 text-xs text-muted-foreground">{{ t("workspace.volumeOutlineHint") }}</p>
            </div>
          </div>
          <div v-else-if="selectedIsChapter" class="flex min-h-0 flex-1 flex-col gap-2">
            <div class="min-h-0 shrink overflow-y-auto space-y-2">
            <div class="space-y-1.5 rounded-md border bg-sky-50/50 p-2 dark:bg-sky-950/20">
              <p class="text-[11px] font-medium text-sky-900 dark:text-sky-100">
                {{ t("workspace.chapterLinkedPlots") }}
              </p>
              <p class="text-[10px] text-muted-foreground">{{ t("workspace.plotReorderHint") }}</p>
              <ul v-if="chapterLinkedPlotsFromRoot.length" class="mb-1.5 space-y-1">
                <li
                  v-for="p in chapterLinkedPlotsFromRoot"
                  :key="'root-' + p.id"
                  class="flex items-center gap-2 rounded border border-dashed bg-muted/40 px-2 py-1.5 text-xs"
                >
                  <span
                    class="shrink-0 rounded bg-muted px-1 py-0.5 text-[9px] font-medium text-muted-foreground"
                    :title="t('workspace.chapterPlotInherited')"
                  >{{ t("workspace.chapterPlotInherited") }}</span>
                  <span class="min-w-0 flex-1 truncate">{{ p.label }}</span>
                </li>
              </ul>
              <ul v-if="chapterLinkedPlotsFromVolume.length" class="mb-1.5 space-y-1">
                <li
                  v-for="p in chapterLinkedPlotsFromVolume"
                  :key="'vol-' + p.id"
                  class="flex items-center gap-2 rounded border border-dashed bg-violet-50/60 px-2 py-1.5 text-xs dark:bg-violet-950/30"
                >
                  <span
                    class="shrink-0 rounded bg-violet-100 px-1 py-0.5 text-[9px] font-medium text-violet-800 dark:bg-violet-900 dark:text-violet-100"
                    :title="t('workspace.volumePlotInherited')"
                  >{{ t("workspace.volumePlotInherited") }}</span>
                  <span class="min-w-0 flex-1 truncate">{{ p.label }}</span>
                </li>
              </ul>
              <ul v-if="chapterLinkedPlotsLocal.length" ref="plotListEl" class="space-y-1">
                <li
                  v-for="(p, i) in chapterLinkedPlotsLocal"
                  :key="p.id"
                  :data-plot-idx="i"
                  class="flex cursor-grab items-center gap-2 rounded border bg-background px-2 py-1.5 text-xs select-none active:cursor-grabbing"
                  :class="{
                    'opacity-40': plotReorderDragFrom === i,
                    'ring-2 ring-sky-500':
                      plotReorderDragFrom !== null &&
                      plotReorderOver === i &&
                      plotReorderDragFrom !== i,
                  }"
                  @pointerdown="onHostPlotPointerDown(i, $event)"
                >
                  <span
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-sky-500 text-[10px] font-medium text-white"
                    :title="t('workspace.plotOrderDot', { i, name: p.label })"
                  >{{ i }}</span>
                  <span class="min-w-0 flex-1 truncate">{{ p.label }}</span>
                </li>
              </ul>
              <p
                v-if="!chapterLinkedPlotsFromRoot.length && !chapterLinkedPlotsFromVolume.length && !chapterLinkedPlotsLocal.length"
                class="text-[10px] text-muted-foreground"
              >{{ t("workspace.chapterLinkedPlotsEmpty") }}</p>
            </div>
            <div class="space-y-1.5 rounded-md border bg-teal-50/50 p-2 dark:bg-teal-950/20">
              <p class="text-[11px] font-medium text-teal-900 dark:text-teal-100">
                {{ t("workspace.chapterLinkedKnowledge") }}
              </p>
              <p class="text-[10px] text-muted-foreground">{{ t("workspace.chapterLinkedKnowledgeHint") }}</p>
              <ul v-if="chapterLinkedKnowledge.length" class="flex flex-wrap items-start justify-start gap-1">
                <li
                  v-for="k in chapterLinkedKnowledge"
                  :key="k.id"
                  class="inline-flex max-w-full items-center gap-1.5 rounded border px-2 py-1 text-xs"
                  :class="k.chipShell"
                >
                  <span
                    v-if="k.from !== 'chapter'"
                    class="mt-0.5 shrink-0 rounded px-1 py-0.5 text-[9px] font-medium"
                    :class="
                      k.from === 'volume'
                        ? 'bg-violet-100 text-violet-800 dark:bg-violet-900 dark:text-violet-100'
                        : 'bg-muted text-muted-foreground'
                    "
                  >{{
                    k.from === "root"
                      ? t("workspace.chapterPlotInherited")
                      : t("workspace.volumePlotInherited")
                  }}</span>
                  <div class="min-w-0">
                    <span class="block max-w-[180px] truncate font-medium">{{ k.label }}</span>
                  </div>
                </li>
              </ul>
              <p v-else class="text-[10px] text-muted-foreground">{{ t("workspace.chapterLinkedKnowledgeEmpty") }}</p>
            </div>
            <div
              v-if="chapterLinkTags.chars.length"
              class="flex flex-wrap gap-1"
            >
              <span
                v-for="name in chapterLinkTags.chars"
                :key="'c-' + name"
                class="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] text-amber-900"
                :title="t('workspace.role')"
              >{{ name }}</span>
            </div>
            </div>
            <div class="flex min-h-[8rem] flex-1 flex-col space-y-2">
              <label class="block shrink-0 text-xs text-muted-foreground">{{ t("workspace.chapterOutline") }}</label>
              <div class="relative min-h-0 flex-1">
                <Textarea
                  :model-value="selected?.outline ?? ''"
                  class="absolute inset-0 resize-none overflow-y-auto text-sm"
                  :placeholder="t('workspace.chapterOutlinePh')"
                  :disabled="chapterBusy"
                  @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                  @change="persistSelectedCardText"
                />
              </div>
              <p class="shrink-0 text-xs text-muted-foreground">{{ t("workspace.chapterOutlineHint") }}</p>
            </div>
          </div>
          <div v-else-if="selected?.kind === 'side_plot'" class="flex min-h-0 flex-1 flex-col space-y-2">
            <label class="block shrink-0 text-xs text-muted-foreground">{{ t("workspace.plotOutline") }}</label>
            <div class="relative min-h-0 flex-1">
              <Textarea
                :model-value="selected?.outline ?? ''"
                class="absolute inset-0 resize-none overflow-y-auto text-sm"
                :placeholder="t('workspace.plotOutlinePh')"
                :disabled="chapterBusy"
                @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                @change="persistSelectedCardText"
              />
            </div>
            <p class="shrink-0 text-xs text-muted-foreground">{{ t("workspace.plotOutlineHint") }}</p>
          </div>
          <div v-else class="text-sm text-muted-foreground">
            {{ t("workspace.clickNode") }}
          </div>
        </div>
        <div
          v-if="chapterResultNotice"
          class="shrink-0 border-t bg-muted/40 px-3 py-2"
        >
          <p class="text-[11px] font-medium text-muted-foreground">{{ t("workspace.resultPanel") }}</p>
          <p class="mt-1 whitespace-pre-wrap text-xs leading-relaxed">{{ chapterResultNotice }}</p>
        </div>
      </div>

      <div
        class="cursor-col-resize bg-border hover:bg-primary/40"
        style="grid-area: v1"
        :title="t('workspace.resize')"
        @mousedown="startResize('left', $event)"
      />

      <!-- 中：树图（缩放 / 拖动画布） -->
      <div
        class="relative min-h-0 min-w-0"
        style="grid-area: mid"
        tabindex="0"
        @keydown="onCanvasKeydown"
      >
        <div
          class="absolute left-2 top-2 bottom-3 z-20 flex max-w-[calc(100%-1rem)] flex-col gap-1 pointer-events-none"
        >
          <div class="flex flex-wrap items-center gap-1.5 pointer-events-auto">
            <!-- 添加卡片：仅图标，悬停看说明 -->
            <div
              class="flex items-center gap-0.5 rounded-md border border-dashed border-primary/45 bg-background/95 p-0.5 shadow-sm"
              :title="t('workspace.addCardsGroup')"
            >
              <span
                class="flex h-7 w-5 items-center justify-center text-primary"
                aria-hidden="true"
              >
                <Plus class="h-3.5 w-3.5" />
              </span>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-muted-foreground hover:bg-primary/10 hover:text-primary"
                :title="t('workspace.addChapter')"
                :aria-label="t('workspace.addChapter')"
                @click="addCard('chapter')"
              >
                <BookOpen class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-violet-800 hover:bg-violet-100 hover:text-violet-900"
                :title="t('workspace.addVolume')"
                :aria-label="t('workspace.addVolume')"
                @click="addCard('volume')"
              >
                <Layers class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-amber-800 hover:bg-amber-100 hover:text-amber-900"
                :title="t('workspace.addCharacter')"
                :aria-label="t('workspace.addCharacter')"
                @click="addCard('character')"
              >
                <User class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-sky-800 hover:bg-sky-100 hover:text-sky-900"
                :title="t('workspace.addPlot')"
                :aria-label="t('workspace.addPlot')"
                @click="addCard('side_plot')"
              >
                <GitBranch class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-teal-800 hover:bg-teal-100 hover:text-teal-900"
                :title="t('workspace.addKnowledge')"
                :aria-label="t('workspace.addKnowledge')"
                @click="addCard('knowledge')"
              >
                <Library class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 w-7 px-0 text-teal-800 hover:bg-teal-100 hover:text-teal-900"
                :title="t('workspace.addPublicKnowledge')"
                :aria-label="t('workspace.addPublicKnowledge')"
                @click="openPublicPick"
              >
                <Share2 class="h-3.5 w-3.5" />
              </Button>
            </div>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-7 bg-background/95 px-0"
              :class="showChapterNav ? 'text-primary' : 'text-muted-foreground'"
              :disabled="!tree"
              :title="t('workspace.chapterNavToggle')"
              :aria-label="t('workspace.chapterNavToggle')"
              :aria-pressed="showChapterNav"
              @click="toggleChapterNav"
            >
              <ListTree class="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-7 bg-background/95 px-0"
              :disabled="!tree"
              :title="t('workspace.autoLayout')"
              :aria-label="t('workspace.autoLayout')"
              @click="autoLayout"
            >
              <LayoutGrid class="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-7 bg-background/95 px-0"
              :disabled="!tree || allMemoryBusy"
              :title="t('workspace.allChapterMemory')"
              :aria-label="t('workspace.allChapterMemory')"
              @click="openAllChapterMemory"
            >
              <Loader2 v-if="allMemoryBusy" class="h-3.5 w-3.5 animate-spin" />
              <Brain v-else class="h-3.5 w-3.5" />
            </Button>
          </div>
          <div
            class="w-fit max-w-full rounded bg-background/90 px-2 py-1 text-[10px] leading-snug text-muted-foreground shadow"
          >
            {{ t("workspace.flowHint") }}
          </div>
          <nav
            v-if="showChapterNav && tree"
            class="flex min-h-0 w-[9.5rem] flex-1 flex-col pointer-events-auto"
            :aria-label="t('workspace.canvasNav')"
          >
            <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-border/70 bg-background/95 shadow-sm backdrop-blur-sm">
              <div class="flex shrink-0 border-b border-border/70" role="tablist" :aria-label="t('workspace.canvasNav')">
                <button
                  v-for="tab in canvasNavTabDefs"
                  :key="tab.id"
                  type="button"
                  role="tab"
                  class="flex flex-1 items-center justify-center px-0 py-1.5 transition-colors"
                  :class="
                    canvasNavTab === tab.id
                      ? 'bg-primary/10 text-primary'
                      : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground'
                  "
                  :title="tab.label"
                  :aria-label="tab.label"
                  :aria-selected="canvasNavTab === tab.id"
                  @click="canvasNavTab = tab.id"
                >
                  <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                </button>
              </div>
              <div class="min-h-0 flex-1 overflow-y-auto py-0.5">
                <button
                  v-for="item in canvasNavActiveItems"
                  :key="item.id"
                  type="button"
                  class="block w-full truncate py-0.5 text-left text-[11px] leading-snug hover:bg-muted/70"
                  :class="[
                    item.indent ? 'pl-3 pr-2' : 'px-2',
                    selected?.id === item.id
                      ? 'bg-primary/10 font-medium text-primary'
                      : item.kind === 'novel'
                        ? 'text-muted-foreground'
                        : 'text-foreground',
                  ]"
                  :title="item.label"
                  @click="focusCanvasNav(item.id)"
                >
                  <span v-if="item.indent" class="text-muted-foreground">|- </span>{{ item.label }}
                </button>
                <p
                  v-if="!canvasNavActiveItems.length"
                  class="px-2 py-2 text-center text-[10px] leading-snug text-muted-foreground"
                >
                  {{ canvasNavEmptyLabel }}
                </p>
              </div>
            </div>
          </nav>
        </div>
        <div
          v-if="relationEdgeId"
          class="absolute left-1/2 top-14 z-20 w-[min(360px,90%)] -translate-x-1/2 rounded-lg border bg-background p-3 shadow-lg"
          @mousedown.stop
          @click.stop
        >
          <p class="mb-1 text-xs font-medium">{{ t("workspace.relationPrompt") }}</p>
          <p v-if="relationPairLabel" class="mb-2 text-[11px] text-muted-foreground">{{ relationPairLabel }}</p>
          <input
            ref="relationInputEl"
            v-model="relationDraft"
            type="text"
            class="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            :placeholder="t('workspace.relationHint')"
            @keydown.enter.prevent="saveRelation"
            @keydown.escape.prevent="closeRelationEditor"
          />
          <div class="mt-2 flex justify-end gap-2">
            <Button size="sm" variant="ghost" @click="closeRelationEditor">{{ t("novels.cancel") }}</Button>
            <Button size="sm" @click="saveRelation">{{ t("workspace.relationSave") }}</Button>
          </div>
        </div>
        <VueFlow
          id="nove-workspace"
          :nodes="flowNodes"
          :edges="flowEdges"
          :node-types="nodeTypes"
          :connection-mode="ConnectionMode.Loose"
          :edges-updatable="true"
          :delete-key-code="flowDeleteKeyCode"
          :default-viewport="{ x: 40, y: 20, zoom: 0.85 }"
          :min-zoom="0.15"
          :max-zoom="2.5"
          :pan-on-drag="true"
          :zoom-on-scroll="true"
          :zoom-on-pinch="true"
          :nodes-draggable="true"
          fit-view-on-init
          class="h-full w-full"
          @node-click="onNodeClick"
          @connect="onConnect"
          @edge-click="onEdgeClick"
          @edge-update="onEdgeUpdate"
          @edge-double-click="onEdgeDoubleClick"
          @node-drag-stop="onNodeDragStop"
        >
          <Background pattern-color="#d5ddd4" :gap="18" />
          <Controls />
          <MiniMap />
        </VueFlow>
      </div>

      <div
        v-if="chatDock === 'right'"
        class="cursor-col-resize bg-border hover:bg-primary/40"
        style="grid-area: v2"
        :title="t('workspace.resize')"
        @mousedown="startResize('right', $event)"
      />

      <div
        v-if="chatDock === 'bottom'"
        class="cursor-row-resize bg-border hover:bg-primary/40"
        style="grid-area: hr"
        :title="t('workspace.resizeChatH')"
        @mousedown="startResizeChatH($event)"
      />

      <!-- 章节正文：右侧栏 或 树图下方（原 Chat 位） -->
      <div
        class="flex min-h-0 min-w-0 flex-col bg-background"
        style="grid-area: chat"
        :class="chatDock === 'right' ? 'border-l' : 'border-t'"
      >
        <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
          <div class="min-w-0 truncate text-sm font-medium">
            {{ t("workspace.bodyDockTitle") }}
          </div>
          <div class="flex min-w-0 flex-wrap items-center justify-end gap-2">
            <template v-if="selectedIsChapter">
              <Button
                v-if="chapterBusy"
                size="sm"
                variant="destructive"
                @click="stopChapter"
              >
                <Square class="mr-1.5 h-3.5 w-3.5" />
                {{ t("workspace.stop") }}
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="px-2"
                :disabled="!!busy || !(bodyEditing ? bodyDraft : chapterMd)"
                :title="t('workspace.copyBody')"
                :aria-label="t('workspace.copyBody')"
                @click="copyChapterBody"
              >
                <Copy class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="px-2"
                :disabled="memoryBusy || (chapterBusy && busy !== 'regen-memory')"
                :title="t('workspace.memoryPreview')"
                :aria-label="t('workspace.memoryPreview')"
                @click="openChapterMemory"
              >
                <Loader2
                  v-if="memoryBusy || busy === 'regen-memory'"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <Brain v-else class="h-3.5 w-3.5" />
              </Button>
              <span v-if="copyHint" class="text-[11px] text-muted-foreground">{{ copyHint }}</span>
            </template>
            <div
              class="inline-flex rounded-md border bg-muted/40 p-0.5"
              role="group"
              :aria-label="t('workspace.bodyDockHint')"
            >
              <button
                type="button"
                class="rounded px-1.5 py-1 transition-colors"
                :class="
                  chatDock === 'right'
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:text-foreground'
                "
                :aria-pressed="chatDock === 'right'"
                :title="t('workspace.bodyDockRight')"
                :aria-label="t('workspace.bodyDockRight')"
                @click="chatDock = 'right'"
              >
                <PanelRight class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="rounded px-1.5 py-1 transition-colors"
                :class="
                  chatDock === 'bottom'
                    ? 'bg-primary text-primary-foreground'
                    : 'text-muted-foreground hover:text-foreground'
                "
                :aria-pressed="chatDock === 'bottom'"
                :title="t('workspace.bodyDockBottom')"
                :aria-label="t('workspace.bodyDockBottom')"
                @click="chatDock = 'bottom'"
              >
                <PanelBottom class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
        <div class="flex min-h-0 flex-1 flex-col overflow-hidden p-3">
          <template v-if="!selectedIsChapter">
            <p class="text-xs text-muted-foreground">{{ t("workspace.bodyDockPickChapter") }}</p>
          </template>
          <template v-else>
            <div
              v-if="hasRefineDiff"
              class="mb-2 flex shrink-0 flex-wrap items-center gap-1 border-b pb-2"
            >
              <button
                type="button"
                class="rounded px-2 py-1 text-xs"
                :class="bodyTab === 'body' ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted'"
                @click="bodyTab = 'body'"
              >
                {{ t("workspace.bodyTab") }}
              </button>
              <button
                type="button"
                class="rounded px-2 py-1 text-xs"
                :class="bodyTab === 'diff' ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted'"
                @click="bodyTab = 'diff'"
              >
                {{ t("workspace.refineDiffTab") }}
              </button>
            </div>
            <div
              v-if="bodyTab === 'diff' && hasRefineDiff"
              class="min-h-0 flex-1 space-y-0.5 overflow-auto font-sans text-sm leading-relaxed"
              :class="chapterBusy ? 'opacity-40' : ''"
            >
              <p class="mb-2 text-[11px] text-muted-foreground">{{ t("workspace.refineDiffHint") }}</p>
              <div
                v-for="(h, i) in refineDiffHunks"
                :key="i"
                class="whitespace-pre-wrap rounded-sm px-1"
                :class="{
                  'bg-red-100 text-red-950 dark:bg-red-950/40 dark:text-red-100': h.type === 'del',
                  'bg-emerald-100 text-emerald-950 dark:bg-emerald-950/40 dark:text-emerald-100': h.type === 'add',
                }"
              >
                <span class="mr-1 select-none opacity-50">{{
                  h.type === "del" ? "−" : h.type === "add" ? "+" : " "
                }}</span>{{ h.text || " " }}
              </div>
            </div>
            <div
              v-else
              class="relative flex min-h-0 w-full flex-1"
              @pointermove="onBodyEditorPointerMove"
              @pointerleave="onBodyEditorPointerLeave"
            >
              <div
                class="relative w-6 shrink-0 self-stretch overflow-hidden"
                :aria-label="t('workspace.paraRewriteGutter')"
              >
                <button
                  v-for="g in paraGutterTops"
                  v-show="hoveredParaIndex === g.index"
                  :key="g.index"
                  type="button"
                  class="absolute left-0.5 z-[1] flex h-5 w-5 items-center justify-center rounded text-amber-800 hover:bg-amber-100 disabled:pointer-events-none disabled:opacity-40 dark:text-amber-200 dark:hover:bg-amber-950"
                  :style="{ top: `${g.top - bodyScrollTop}px` }"
                  :title="t('workspace.paraRewrite')"
                  :aria-label="t('workspace.paraRewrite')"
                  :disabled="paraRewriteBusy || bodySaveBusy"
                  @click="openParaRewrite(g.index)"
                >
                  <Sparkles class="h-3.5 w-3.5" />
                </button>
              </div>
              <div class="relative min-h-0 min-w-0 flex-1">
                <textarea
                  ref="bodyTaEl"
                  v-model="bodyDraft"
                  class="absolute inset-0 resize-none overflow-y-auto rounded-md border border-input bg-transparent px-3 py-2 font-sans text-sm leading-relaxed shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  :disabled="bodySaveBusy || paraRewriteBusy"
                  :placeholder="t('workspace.editBodyPh')"
                  :aria-label="t('workspace.editBody')"
                  @scroll="onBodyTaScroll"
                />
                <!-- 与 textarea 同宽同排版，用于量段落首行 top -->
                <div
                  ref="bodyMirrorEl"
                  class="pointer-events-none invisible absolute left-0 top-0 -z-10 px-3 py-2 font-sans text-sm leading-relaxed"
                  aria-hidden="true"
                >
                  <div
                    v-for="(line, i) in bodyLines"
                    :key="i"
                    :data-para-i="i"
                    :data-nonempty="line.trim() ? '1' : '0'"
                    class="whitespace-pre-wrap break-words"
                  >{{ line || "\u00a0" }}</div>
                </div>
              </div>
            </div>
          </template>
        </div>
        <div
          v-if="selectedIsChapter && bodyEditing && !(bodyTab === 'diff' && hasRefineDiff) && (bodySaveBusy || bodyAutosaved)"
          class="flex shrink-0 items-center border-t px-3 py-1.5"
        >
          <span class="text-[11px] text-muted-foreground">{{
            bodySaveBusy ? t("workspace.bodySaving") : t("workspace.bodyAutosaved")
          }}</span>
        </div>
      </div>
    </div>

    <!-- 段落 AI 改写 -->
    <div
      v-if="pendingParaRewrite"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelParaRewrite"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.paraRewriteTitle") }}</h2>
        <p class="mt-2 max-h-28 overflow-y-auto whitespace-pre-wrap rounded border bg-muted/40 px-2 py-1.5 text-xs text-muted-foreground">
          {{ pendingParaRewrite.text }}
        </p>
        <label class="mt-3 block text-xs text-muted-foreground">{{ t("workspace.paraRewriteNote") }}</label>
        <Textarea
          v-model="paraRewriteNote"
          rows="4"
          class="mt-1 text-sm"
          :disabled="paraRewriteBusy"
          :placeholder="t('workspace.paraRewriteNotePh')"
        />
        <p v-if="paraRewriteError" class="mt-2 text-xs text-destructive">{{ paraRewriteError }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="outline" :disabled="paraRewriteBusy" @click="cancelParaRewrite">
            {{ t("novels.cancel") }}
          </Button>
          <Button
            size="sm"
            :disabled="paraRewriteBusy || !paraRewriteNote.trim()"
            @click="confirmParaRewrite"
          >
            <Loader2 v-if="paraRewriteBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            <Sparkles v-else class="mr-1.5 h-3.5 w-3.5" />
            {{ paraRewriteBusy ? t("workspace.paraRewriteBusy") : t("workspace.paraRewriteRun") }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 删除卡片确认（有正文的章节为两步） -->
    <div
      v-if="pendingCardDelete"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelPendingCardDelete"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.deleteCard") }}</h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{
            pendingCardDelete.isVolume
              ? pendingCardDelete.step === 2
                ? t("workspace.deleteVolumeConfirmAgain", { title: pendingCardDelete.title })
                : t("workspace.deleteVolumeConfirm", { title: pendingCardDelete.title })
              : pendingCardDelete.step === 2
                ? t("workspace.deleteChapterConfirmAgain", { title: pendingCardDelete.title })
                : t("workspace.deleteCardConfirm", { title: pendingCardDelete.title })
          }}
        </p>
        <p
          v-if="pendingCardDelete.isVolume && pendingCardDelete.step === 1"
          class="mt-2 text-sm font-medium text-muted-foreground"
        >
          {{ t("workspace.deleteVolumeReparentHint") }}
        </p>
        <p
          v-else-if="pendingCardDelete.hasBody && pendingCardDelete.step === 1"
          class="mt-2 text-sm font-medium text-destructive"
        >
          {{ t("workspace.deleteChapterHasBodyHint") }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="deletingCard" @click="cancelPendingCardDelete">
            {{ t("novels.cancel") }}
          </Button>
          <Button variant="destructive" :disabled="deletingCard" @click="confirmPendingCardDelete">
            {{
              deletingCard
                ? t("workspace.deletingCard")
                : (pendingCardDelete.hasBody || pendingCardDelete.isVolume) &&
                    pendingCardDelete.step === 1
                  ? t("workspace.deleteContinue")
                  : t("workspace.deleteCard")
            }}
          </Button>
        </div>
      </div>
    </div>

    <div
      v-if="pendingPublishDup"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelPublishDup"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.publishKnowledgeDupTitle") }}</h2>
        <p class="mt-3 text-sm text-muted-foreground">
          {{ t("workspace.publishKnowledgeDupBody", { title: pendingPublishDup.draft.title }) }}
        </p>
        <div class="mt-5 flex flex-wrap justify-end gap-2">
          <Button variant="outline" :disabled="publishBusy" @click="cancelPublishDup">
            {{ t("novels.cancel") }}
          </Button>
          <Button
            variant="outline"
            :disabled="publishBusy"
            @click="commitPublish(null, pendingPublishDup.draft)"
          >
            {{ t("workspace.publishKnowledgeDupNew") }}
          </Button>
          <Button
            :disabled="publishBusy"
            @click="commitPublish(pendingPublishDup.existingId, pendingPublishDup.draft)"
          >
            {{ publishBusy ? t("workspace.publishKnowledge") : t("workspace.publishKnowledgeDupOverwrite") }}
          </Button>
        </div>
      </div>
    </div>

    <WorldviewChatPanel
      :open="worldviewChatOpen"
      :novel-id="props.id"
      @close="worldviewChatOpen = false"
      @applied="onWorldviewChatApplied"
    />

    <div
      v-if="publicPickOpen"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="publicPickOpen = false"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.pickPublicKnowledge") }}</h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ t("workspace.pickPublicKnowledgeHint") }}</p>
        <Input
          v-model="publicPickFilter"
          class="mt-3 h-8 text-xs"
          :placeholder="t('workspace.pickPublicKnowledgeSearch')"
        />
        <p v-if="!filteredPublicCards.length && !publicPickFilter.trim()" class="mt-3 text-sm text-muted-foreground">
          {{ t("workspace.noPublicKnowledge") }}
        </p>
        <div v-else class="mt-3 max-h-64 overflow-y-auto rounded-md border">
          <button
            v-for="c in filteredPublicCards"
            :key="c.id"
            type="button"
            class="flex w-full flex-col items-start gap-0.5 border-b border-border/50 px-3 py-2 text-left last:border-b-0 hover:bg-muted/50 disabled:opacity-50"
            :disabled="publicPickBusy"
            @click="addPickedPublicCard(c.id)"
          >
            <span class="text-sm font-medium">{{ c.title }}</span>
            <span class="line-clamp-2 text-[11px] text-muted-foreground">{{
              (c.extracted || "").trim() || t("workspace.knowledgePending")
            }}</span>
          </button>
          <p
            v-if="filteredPublicCards.length === 0 && publicPickFilter.trim()"
            class="px-3 py-4 text-center text-xs text-muted-foreground"
          >
            {{ t("workspace.pickPublicKnowledgeEmpty") }}
          </p>
        </div>
        <div class="mt-4 flex justify-end">
          <Button variant="outline" :disabled="publicPickBusy" @click="publicPickOpen = false">
            {{ t("novels.cancel") }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 章节记忆：已提取 | 抽取条件（横排）+ 底部按钮 -->
    <div
      v-if="memoryPanelOpen"
      class="fixed inset-0 z-50 flex items-stretch justify-center bg-black/30 p-0 sm:p-4 md:p-6"
      @click.self="closeChapterMemory"
      @keydown.escape="closeChapterMemory"
    >
      <div
        class="flex h-full w-full max-w-6xl flex-col border bg-background shadow-lg sm:h-[min(92vh,860px)] sm:rounded-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="t('workspace.memoryPanelTitle')"
      >
        <div class="flex shrink-0 items-center justify-between gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">{{ t("workspace.memoryPanelTitle") }}</h2>
            <p class="truncate text-xs text-muted-foreground">{{ selected?.label }}</p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            class="px-2"
            :disabled="busy === 'regen-memory'"
            :aria-label="t('workspace.memoryClose')"
            @click="closeChapterMemory"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex min-h-0 flex-1 flex-col gap-3 p-4 md:flex-row md:gap-4">
          <!-- 其他章记忆（勾选作对照） -->
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <div
              class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2"
            >
              <p class="min-w-0 text-xs font-medium text-muted-foreground">
                {{ t("workspace.memoryRefPicksLabel") }}
              </p>
              <div
                v-if="!memoryRefLoading && memoryRefPicks.length"
                class="flex shrink-0 gap-1"
              >
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  :disabled="busy === 'regen-memory'"
                  @click="selectAllMemoryRefs"
                >
                  {{ t("workspace.generateMemorySelectAll") }}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  :disabled="busy === 'regen-memory'"
                  @click="deselectAllMemoryRefs"
                >
                  {{ t("workspace.generateMemoryDeselectAll") }}
                </Button>
              </div>
            </div>
            <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3">
              <div
                v-if="memoryRefLoading"
                class="flex items-center gap-2 text-xs text-muted-foreground"
              >
                <Loader2 class="h-3.5 w-3.5 animate-spin" />
                {{ t("workspace.memoryRefPicksLoading") }}
              </div>
              <p
                v-else-if="!memoryRefPicks.length"
                class="text-xs text-muted-foreground"
              >
                {{ t("workspace.memoryRefPicksEmpty") }}
              </p>
              <ul v-else class="space-y-2">
                <li
                  v-for="c in memoryRefPicks"
                  :key="c.node_id"
                  class="flex items-start gap-2 rounded-md border bg-background px-3 py-2 text-xs"
                >
                  <input
                    :id="'mem-ref-' + c.node_id"
                    type="checkbox"
                    class="mt-0.5 h-3.5 w-3.5 shrink-0"
                    :disabled="busy === 'regen-memory'"
                    :checked="memoryRefSelectedIds.includes(c.node_id)"
                    @change="toggleMemoryRefId(c.node_id)"
                  />
                  <label :for="'mem-ref-' + c.node_id" class="min-w-0 flex-1 cursor-pointer">
                    <span class="font-medium">{{ c.label }}</span>
                    <span
                      class="mt-0.5 block text-[11px]"
                      :class="
                        c.has_memory
                          ? 'text-muted-foreground'
                          : 'text-amber-700 dark:text-amber-400'
                      "
                    >
                      {{
                        c.has_memory
                          ? t("workspace.generateMemoryHas")
                          : t("workspace.memoryRefMissing")
                      }}
                    </span>
                  </label>
                </li>
              </ul>
            </div>
          </div>
          <!-- 已提取记忆 -->
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <p class="shrink-0 border-b px-3 py-2 text-xs font-medium text-muted-foreground">
              {{ t("workspace.memoryExtractedLabel") }}
            </p>
            <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3">
              <div v-if="memoryBusy" class="flex items-center gap-2 text-xs text-muted-foreground">
                <Loader2 class="h-3.5 w-3.5 animate-spin" />
                {{ t("workspace.memoryLoading") }}
              </div>
              <template v-else>
                <p v-if="memoryError" class="mb-3 text-xs text-destructive">{{ memoryError }}</p>
                <p v-if="!memoryItems.length" class="text-xs text-muted-foreground">
                  {{ t("workspace.memoryEmpty") }}
                </p>
                <ul v-else class="space-y-3">
                  <li
                    v-for="(item, i) in memoryItems"
                    :key="i"
                    class="rounded-md border bg-background px-3 py-2 text-xs leading-relaxed whitespace-pre-wrap"
                    :class="
                      item.includes('整体情节') || item.toLowerCase().includes('overall plot')
                        ? 'border-primary/30'
                        : ''
                    "
                  >
                    {{ item }}
                  </li>
                </ul>
              </template>
            </div>
          </div>
          <!-- 抽取条件 -->
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <div
              class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2"
            >
              <label
                class="min-w-0 text-xs font-medium text-muted-foreground"
                for="memory-extract-notes"
              >
                {{ t("workspace.memoryExtractNotesLabel") }}
              </label>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 shrink-0 px-2 text-xs"
                :disabled="busy === 'regen-memory'"
                :title="t('workspace.memoryExtractResetHint')"
                @click="resetMemoryExtractNotes"
              >
                {{ t("workspace.memoryExtractReset") }}
              </Button>
            </div>
            <Textarea
              id="memory-extract-notes"
              v-model="memoryExtractNotes"
              class="min-h-0 flex-1 resize-none rounded-none border-0 bg-transparent text-xs leading-relaxed shadow-none focus-visible:ring-0"
              :disabled="busy === 'regen-memory'"
              :placeholder="t('workspace.memoryExtractNotesPh')"
              :aria-label="t('workspace.memoryExtractNotesLabel')"
            />
          </div>
        </div>
        <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t px-4 py-3">
          <p class="min-w-0 flex-1 text-[11px] text-muted-foreground">
            {{ t("workspace.memoryExtractHint") }}
          </p>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Button
              v-if="busy === 'regen-memory'"
              size="sm"
              variant="destructive"
              @click="stopChapter"
            >
              <Square class="mr-1.5 h-3.5 w-3.5" />
              {{ t("workspace.stop") }}
            </Button>
            <Button
              size="sm"
              :disabled="!!busy || !chapterMd.trim()"
              @click="regenerateChapterMemory"
            >
              <Loader2 v-if="busy === 'regen-memory'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              <RefreshCw v-else class="mr-1.5 h-3.5 w-3.5" />
              {{
                busy === "regen-memory"
                  ? t("workspace.regenMemoryBusy")
                  : memoryItems.length
                    ? t("workspace.regenMemory")
                    : t("workspace.extractMemory")
              }}
            </Button>
          </div>
        </div>
      </div>
    </div>

    <!-- 删除章节记忆确认 -->
    <div
      v-if="pendingMemoryDelete"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelPendingMemoryDelete"
      @keydown.escape="cancelPendingMemoryDelete"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.deleteMemoryTitle") }}</h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{
            pendingMemoryDelete.itemIndex === null
              ? t("workspace.clearChapterMemoryConfirm", { label: pendingMemoryDelete.label })
              : t("workspace.deleteMemoryItemConfirm", { label: pendingMemoryDelete.label })
          }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="deletingMemory" @click="cancelPendingMemoryDelete">
            {{ t("novels.cancel") }}
          </Button>
          <Button variant="destructive" :disabled="deletingMemory" @click="confirmPendingMemoryDelete">
            {{ deletingMemory ? t("workspace.deletingMemory") : t("workspace.deleteMemoryTitle") }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 全书章节记忆 -->
    <div
      v-if="allMemoryOpen"
      class="fixed inset-0 z-50 flex items-stretch justify-center bg-black/30 p-0 sm:p-6"
      @click.self="closeAllChapterMemory"
      @keydown.escape="closeAllChapterMemory"
    >
      <div
        class="flex h-full w-full max-w-5xl flex-col border bg-background shadow-lg sm:h-[min(85vh,720px)] sm:rounded-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="t('workspace.allChapterMemoryTitle')"
      >
        <div class="flex items-center justify-between gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">{{ t("workspace.allChapterMemoryTitle") }}</h2>
            <p class="truncate text-xs text-muted-foreground">
              {{ t("workspace.allChapterMemoryHint") }}
            </p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            class="px-2"
            :aria-label="t('workspace.memoryClose')"
            @click="closeAllChapterMemory"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex min-h-0 flex-1 flex-col sm:flex-row">
          <div class="min-h-0 w-full shrink-0 overflow-y-auto border-b sm:w-72 sm:border-b-0 sm:border-r">
            <div v-if="allMemoryBusy" class="flex items-center gap-2 p-4 text-xs text-muted-foreground">
              <Loader2 class="h-3.5 w-3.5 animate-spin" />
              {{ t("workspace.memoryLoading") }}
            </div>
            <p v-else-if="allMemoryError" class="p-4 text-xs text-destructive">{{ allMemoryError }}</p>
            <p v-else-if="!allMemoryGroups.length" class="p-4 text-xs text-muted-foreground">
              {{ t("workspace.allChapterMemoryEmpty") }}
            </p>
            <ul v-else class="divide-y p-1">
              <li v-for="g in allMemoryGroups" :key="g.node_id">
                <button
                  type="button"
                  class="w-full rounded-md px-3 py-2.5 text-left transition-colors hover:bg-muted/60"
                  :class="
                    allMemorySelectedId === g.node_id
                      ? 'bg-accent text-accent-foreground'
                      : ''
                  "
                  @click="selectAllMemoryChapter(g.node_id)"
                >
                  <div class="truncate text-sm font-medium">{{ g.label }}</div>
                  <div class="mt-0.5 text-[11px] text-muted-foreground">
                    {{ t("workspace.memoryItemCount", { n: g.items.length }) }}
                  </div>
                </button>
              </li>
            </ul>
          </div>
          <div class="flex min-h-0 min-w-0 flex-1 flex-col">
            <template v-if="allMemorySelectedGroup">
              <div class="flex shrink-0 items-start justify-between gap-2 border-b px-4 py-2">
                <div class="min-w-0">
                  <p class="truncate text-sm font-medium">{{ allMemorySelectedGroup.label }}</p>
                  <p class="text-[11px] text-muted-foreground">{{ t("workspace.allChapterMemoryBody") }}</p>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 shrink-0 px-2 text-destructive hover:bg-destructive/10 hover:text-destructive"
                  :disabled="deletingMemory"
                  :title="t('workspace.clearChapterMemory')"
                  :aria-label="t('workspace.clearChapterMemory')"
                  @click="askClearChapterMemory"
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </Button>
              </div>
              <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3">
                <div
                  v-if="allMemoryBodyBusy"
                  class="flex items-center gap-2 text-xs text-muted-foreground"
                >
                  <Loader2 class="h-3.5 w-3.5 animate-spin" />
                  {{ t("workspace.memoryLoading") }}
                </div>
                <template v-else>
                  <div class="mb-4 space-y-2">
                    <p class="text-[11px] font-medium text-muted-foreground">
                      {{ t("workspace.memoryPanelTitle") }}
                    </p>
                    <ul class="space-y-2">
                      <li
                        v-for="(item, i) in allMemorySelectedGroup.items"
                        :key="i"
                        class="group relative rounded-md border bg-muted/30 px-3 py-2 pr-9 text-xs leading-relaxed whitespace-pre-wrap"
                        :class="
                          item.includes('整体情节') || item.toLowerCase().includes('overall plot')
                            ? 'border-primary/30'
                            : ''
                        "
                      >
                        {{ item }}
                        <Button
                          size="sm"
                          variant="ghost"
                          class="absolute right-1 top-1 h-6 w-6 px-0 text-muted-foreground opacity-70 hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100"
                          :disabled="deletingMemory"
                          :title="t('workspace.deleteMemoryItem')"
                          :aria-label="t('workspace.deleteMemoryItem')"
                          @click="askDeleteMemoryItem(i)"
                        >
                          <Trash2 class="h-3 w-3" />
                        </Button>
                      </li>
                    </ul>
                  </div>
                  <Separator class="my-3" />
                  <p class="mb-2 text-[11px] font-medium text-muted-foreground">
                    {{ t("workspace.bodyTab") }}
                  </p>
                  <p
                    v-if="!allMemoryBody.trim()"
                    class="text-xs text-muted-foreground"
                  >
                    {{ t("workspace.allChapterMemoryNoBody") }}
                  </p>
                  <div
                    v-else
                    class="prose prose-sm max-w-none text-sm leading-relaxed whitespace-pre-wrap"
                  >
                    {{ stripChapterMeta(allMemoryBody) }}
                  </div>
                </template>
              </div>
            </template>
            <p v-else class="p-4 text-xs text-muted-foreground">
              {{ t("workspace.allChapterMemoryPick") }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
