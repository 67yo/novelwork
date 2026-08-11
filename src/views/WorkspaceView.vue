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
import { useLocalStorage } from "@vueuse/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  extractKnowledgeCard,
  type ChapterMemoryGroup,
  type ChatMessage,
  type KnowledgeBook,
  type NovelProject,
  type NovelTree,
  type SkillPreviewItem,
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
import { useI18n } from "@/i18n";
import { usePersistedChatModel } from "@/lib/chatModel";
import {
  BookOpen,
  Brain,
  Copy,
  GitBranch,
  LayoutGrid,
  Library,
  Loader2,
  PanelBottom,
  PanelRight,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Sparkles,
  Square,
  Trash2,
  User,
  X,
} from "@lucide/vue";
import { diffLines } from "@/lib/linediff";
import { renderChapterMd, stripChapterMeta } from "@/lib/md";
import { applyAutoLayout } from "@/lib/treeLayout";
import { formatChatContent } from "@/lib/chatFormat";
import { appendChatRunStats, createChatRunProgress } from "@/lib/chatRunProgress";

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
const knowledgeBooks = ref<KnowledgeBook[]>([]);
const knowledgeBookFilter = ref("");
const knowledgeExtractBusy = ref(false);
const chapterMd = ref("");
/** 章节正文手动编辑 */
const bodyEditing = ref(false);
const bodyDraft = ref("");
const bodySaveBusy = ref(false);
const messages = ref<ChatMessage[]>([]);
const chatInput = ref("");
const chatListEl = ref<HTMLElement | null>(null);
const {
  model: chatModel,
  options: chatModelOptions,
  load: loadChatModel,
  persist: persistChatModel,
} = usePersistedChatModel("chat_model");
/** per-card chat history（落库；按 node 缓存，切走再切回不丢） */
const cardChats = ref<Record<string, { id: string; role: string; content: string }[]>>({});
/** 已从 DB 拉过的卡片，避免 selectNode 反复覆盖内存（含进行中 pending） */
const cardChatHydrated = ref<Record<string, true>>({});
/** 正在执行卡片 Chat 的 nodeId；该节点加载时不得覆盖进行中气泡 */
let cardChatBusyNodeId: string | null = null;
const cardChatInput = ref("");
const prevN = ref(10);
/** 根节点：一次生成多少章章节卡（1–100，且不超过全书章数） */
const rootGenChapterCount = useLocalStorage("novework.rootGenChapterCount", 10);
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
/** 全章自动：预生成 → 精修，可点按钮中止 */
const autoAll = ref(false);
/** 预生成/精修结束后的结束语（左侧底部独立区） */
const chapterResultNotice = ref("");
const coverBusy = ref(false);
const coverPromptBusy = ref(false);
const coverPrompt = ref("");
const coverPromptHint = ref("");
const chapterBodyEl = ref<HTMLElement | null>(null);
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

/** 丢掉指向已删节点的边与 linked_*，避免幽灵关联。 */
function pruneTreeRefs(tr: NovelTree) {
  const alive = new Set(tr.nodes.map((n) => n.id));
  tr.edges = tr.edges.filter((e) => alive.has(e.source) && alive.has(e.target));
  for (const n of tr.nodes) {
    n.linked_character_ids = (n.linked_character_ids ?? []).filter((id) => alive.has(id));
    n.linked_side_plot_ids = (n.linked_side_plot_ids ?? []).filter((id) => alive.has(id));
    n.linked_knowledge_ids = (n.linked_knowledge_ids ?? []).filter((id) => alive.has(id));
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
    updatable: true,
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
  if (hasKnowledge && (hasChapter || hasNovel)) return "knowledge";
  if (hasChar && (hasPlot || hasChapter || hasNovel)) return "character";
  if (hasPlot && (hasChapter || hasNovel)) return "side_plot";
  return "chapter";
}

async function loadAll() {
  novel.value = await api.getNovel(props.id);
  tree.value = await api.getTree(props.id);
  knowledgeBooks.value = (await api.listKnowledge()).filter((b) => !b.archived);
  syncFlowFromTree();
  messages.value = await api.listChat(props.id);
  cardChats.value = {};
  cardChatHydrated.value = {};
  cardChatBusyNodeId = null;
  const root = tree.value.nodes.find((n) => n.kind === "novel");
  const firstChapter = tree.value.nodes.find((n) => n.kind === "chapter");
  if (firstChapter) await selectNode(firstChapter);
  else if (root) await selectNode(root);
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
  bodyTab.value = "body";
  bodyEditing.value = false;
  bodyDraft.value = "";
  if (!autoAll.value) notice.value = "";
  chapterResultNotice.value = "";
  copyHint.value = "";
  memoryPanelOpen.value = false;
  if (n.kind === "chapter" || n.kind === "side_plot") {
    chapterMd.value = await api.getChapter(props.id, n.id);
  } else {
    chapterMd.value = "";
  }
  if (n.kind === "chapter" || n.kind === "character" || n.kind === "side_plot") {
    void loadCardChatHistory(n.id);
  }
}

function startEditChapterBody() {
  if (!selected.value || selected.value.kind !== "chapter" || chapterBusy.value || autoAll.value) {
    return;
  }
  bodyDraft.value = stripChapterMeta(chapterMd.value);
  bodyEditing.value = true;
  bodyTab.value = "body";
}

function cancelEditChapterBody() {
  bodyEditing.value = false;
  bodyDraft.value = "";
}

async function saveChapterBody() {
  if (!selected.value || selected.value.kind !== "chapter" || bodySaveBusy.value) return;
  const nodeId = selected.value.id;
  bodySaveBusy.value = true;
  try {
    const words = await api.saveChapter(props.id, nodeId, bodyDraft.value);
    chapterMd.value = bodyDraft.value.trim() ? `${bodyDraft.value.trim()}\n` : "";
    bodyEditing.value = false;
    bodyDraft.value = "";
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const cur = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (cur) selected.value = cur;
    }
    chapterResultNotice.value = t("workspace.bodySaved", { n: words });
  } catch (e) {
    chapterResultNotice.value = String(e);
  } finally {
    bodySaveBusy.value = false;
  }
}

function onNodeClick(ev: NodeMouseEvent) {
  const data = ev.node.data as TreeNode;
  if (data) void selectNode(data);
}

function unlinkEdgeRefs(e: TreeEdge) {
  if (!tree.value) return;
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
    if (plot && !host.linked_side_plot_ids.includes(plot.id)) {
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
  check_canon: "workspace.taskStepCheckCanon",
  repair_canon: "workspace.taskStepRepairCanon",
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
      v !== "gen-cards" &&
      v !== "sync-canon")
  ) {
    if (!autoAll.value) clearChapterTaskUi();
  }
});

async function loadChatSkills() {
  try {
    const p = await api.listChatSkills();
    chatSkills.value = p.skills;
  } catch {
    chatSkills.value = [];
  }
}

onMounted(async () => {
  await loadAll();
  void loadChatSkills();
  void loadChatModel(t("settings.deprecated"));
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
});
onUnmounted(() => {
  unlistenChapterProgress?.();
  unlistenChapterProgress = null;
  unlistenChapterTokens?.();
  unlistenChapterTokens = null;
  stopChapterTick();
});
watch(() => props.id, loadAll);
watch(locale, () => syncFlowFromTree());

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
  if (!autoAll.value) notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.generateChapter(props.id, nodeId, memoryNodeIds, userBrief);
    delete refineBeforeByNode.value[nodeId];
    refineBeforeByNode.value = { ...refineBeforeByNode.value };
    chapterMd.value = r.content;
    bodyTab.value = "body";
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

function factoryMemoryExtractNotes(): string {
  return t("workspace.memoryExtractNotesDefault");
}

function ensureMemoryExtractNotes() {
  if (!memoryExtractNotes.value.trim()) {
    memoryExtractNotes.value = factoryMemoryExtractNotes();
  }
}

function resetMemoryExtractNotes() {
  if (busy.value === "regen-memory" || autoAll.value) return;
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
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value || autoAll.value) return;
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
  if (!autoAll.value) notice.value = "";
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

/** 左侧「生成章节卡」：只读材料预览 + 可编辑期望（材料服务端组装） */
const pendingOutlineGen = ref<{
  count: number;
  from: number;
  to: number;
  materials: string;
  expectation: string;
  materialsOpen: boolean;
  loading: boolean;
  error: string;
} | null>(null);

async function openRootGenBrief() {
  if (!selected.value || selected.value.kind !== "novel" || !!busy.value || autoAll.value) return;
  if (pendingOutlineGen.value) return;
  const maxN = Math.min(100, Math.max(1, novel.value?.chapter_count || 100));
  const n = Math.min(maxN, Math.max(1, Number(rootGenChapterCount.value) || 1));
  rootGenChapterCount.value = n;
  pendingOutlineGen.value = {
    count: n,
    from: 1,
    to: n,
    materials: "",
    expectation: "",
    materialsOpen: false,
    loading: true,
    error: "",
  };
  try {
    const r = await api.previewChapterOutlineBrief(props.id, "root", n);
    if (!pendingOutlineGen.value) return;
    pendingOutlineGen.value = {
      ...pendingOutlineGen.value,
      materials: r.brief,
      from: r.from,
      to: r.to,
      loading: false,
    };
  } catch (e) {
    pendingOutlineGen.value = null;
    chapterResultNotice.value = String(e);
  }
}

function cancelOutlineGen() {
  pendingOutlineGen.value = null;
}

async function confirmOutlineGen() {
  const p = pendingOutlineGen.value;
  if (!p || p.loading) return;
  // 只传期望；服务端 assemble_outline_gen_brief 再合并
  const brief = p.expectation.trim();
  const count = p.count;
  pendingOutlineGen.value = null;

  busy.value = "gen-cards";
  beginChapterTask("context", 1, 3);
  notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.generateChapterCards(props.id, count, brief);
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const cur = tree.value?.nodes.find((x) => x.id === selected.value!.id);
      if (cur) selected.value = cur;
    }
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
  } finally {
    endChapterTask();
    busy.value = "";
  }
}

async function stopChapter() {
  if (!chapterBusy.value && !autoAll.value) return;
  if (autoAll.value) {
    autoAll.value = false;
    notice.value = t("workspace.autoAllStopped");
  }
  try {
    await api.chatCancel(props.id);
  } catch {
    /* ignore */
  }
}

function stopAutoAll() {
  if (!autoAll.value) return;
  autoAll.value = false;
  notice.value = t("workspace.autoAllStopped");
  void api.chatCancel(props.id).catch(() => {});
}

async function runAutoAll() {
  if (autoAll.value || busy.value || !tree.value) return;
  const chapters = tree.value.nodes
    .filter((n) => n.kind === "chapter")
    .sort((a, b) => a.position.y - b.position.y || a.position.x - b.position.x);
  if (!chapters.length) {
    notice.value = t("workspace.autoAllEmpty");
    return;
  }
  autoAll.value = true;
  let failed = false;
  try {
    for (let i = 0; i < chapters.length; i++) {
      if (!autoAll.value) break;
      const id = chapters[i].id;
      const node = tree.value?.nodes.find((n) => n.id === id);
      if (!node || node.kind !== "chapter") continue;
      notice.value = t("workspace.autoAllProgress", {
        i: i + 1,
        n: chapters.length,
        title: node.label,
      });
      await selectNode(node);
      if (!autoAll.value) break;
      if (!(await generate())) {
        failed = true;
        break;
      }
      if (!autoAll.value) break;
      if (!(await refine())) {
        failed = true;
        break;
      }
    }
    if (autoAll.value && !failed) notice.value = t("workspace.autoAllDone");
  } finally {
    autoAll.value = false;
  }
}

async function copyChapterBody() {
  const md = stripChapterMeta(chapterMd.value);
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

async function scrollChatBottom() {
  await nextTick();
  if (chatListEl.value) chatListEl.value.scrollTop = chatListEl.value.scrollHeight;
}

const CHAT_STEPS: Record<string, MessageKey> = {
  context: "workspace.chatStepContext",
  thinking: "workspace.chatStepThinking",
  skill: "workspace.chatStepSkill",
  tool: "workspace.chatStepTool",
  fetch_web: "workspace.chatStepFetchWeb",
  apply_character: "workspace.chatStepApplyCharacter",
  apply_outlines: "workspace.chatStepApplyOutlines",
  apply_card: "workspace.chatStepApplyCard",
  saving: "workspace.chatStepSaving",
};

function chatProgressLabel(step: string): string | null {
  const key = CHAT_STEPS[step] ?? CHAPTER_STEPS[step];
  return key ? t(key) : null;
}

function patchPending(pendingId: string, lines: string[]) {
  const i = messages.value.findIndex((m) => m.id === pendingId);
  if (i < 0) return;
  const next = [...messages.value];
  next[i] = { ...next[i], content: lines.join("\n") };
  messages.value = next;
}

function patchCardPending(nodeId: string, pendingId: string, lines: string[]) {
  const list = cardMessagesFor(nodeId).map((m) =>
    m.id === pendingId ? { ...m, content: lines.join("\n") } : m,
  );
  setCardMessages(nodeId, list);
}

function startChatRunProgress(onLines: (lines: string[]) => void) {
  return createChatRunProgress({
    initialLabel: t("workspace.chatStepAnalyzing"),
    formatMs: formatStepMs,
    formatPrompt: (n, confirmed) =>
      t(confirmed ? "workspace.taskPromptTokens" : "workspace.taskPromptTokensEst", {
        n: n.toLocaleString(),
      }),
    formatCompletion: (n) => t("workspace.taskCompletionTokens", { n: n.toLocaleString() }),
    formatTotal: (time) => t("workspace.taskTotalTime", { t: time }),
    onLines: (lines) => {
      onLines(lines);
      void scrollChatBottom();
    },
  });
}

function attachChatRunStats<T extends { role: string; content: string }>(
  list: T[],
  stats: string,
): T[] {
  if (!stats.trim()) return list;
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i].role === "assistant") {
      const next = [...list];
      next[i] = { ...next[i], content: appendChatRunStats(next[i].content, stats) };
      return next;
    }
  }
  return list;
}

/** 与后端 slash_args 对齐：匹配根 Chat 清空类指令。 */
function matchRootSlash(text: string, names: string[]): boolean {
  const t = text.trim();
  if (!t.startsWith("/")) return false;
  const rest = t.slice(1);
  const sorted = [...names].sort((a, b) => b.length - a.length);
  for (const name of sorted) {
    if (!rest.startsWith(name)) continue;
    const after = rest.slice(name.length);
    if (after === "" || /^\s/.test(after)) return true;
  }
  return false;
}

function isClearChaptersCmd(text: string): boolean {
  return matchRootSlash(text, [
    "清空章节",
    "清空所有章节",
    "清空所有章节节点",
    "clear-chapters",
    "clear_chapters",
  ]);
}

function isClearPlotsCmd(text: string): boolean {
  return matchRootSlash(text, [
    "清空所有剧情",
    "清空剧情",
    "清空所有剧情卡",
    "清空剧情卡",
    "clear-plots",
    "clear_plots",
  ]);
}

type PendingChatClear = { kind: "chapters" | "plots"; text: string; step: 1 | 2 };
const pendingChatClear = ref<PendingChatClear | null>(null);

function cancelPendingChatClear() {
  pendingChatClear.value = null;
}

function confirmPendingChatClear() {
  const p = pendingChatClear.value;
  if (!p) return;
  if (p.step === 1) {
    pendingChatClear.value = { ...p, step: 2 };
    return;
  }
  const text = p.text;
  pendingChatClear.value = null;
  void runSendChat(text);
}

async function sendChat() {
  const text = chatInput.value.trim();
  if (!text || busy.value === "chat" || pendingChatClear.value) return;
  if (isClearChaptersCmd(text)) {
    pendingChatClear.value = { kind: "chapters", text, step: 1 };
    return;
  }
  if (isClearPlotsCmd(text)) {
    pendingChatClear.value = { kind: "plots", text, step: 1 };
    return;
  }
  await runSendChat(text);
}

async function runSendChat(text: string) {
  if (!text || busy.value === "chat") return;
  busy.value = "chat";
  notice.value = "";
  const now = new Date().toISOString();
  const pendingId = `pending-${Date.now()}`;
  messages.value = [
    ...messages.value,
    {
      id: `local-user-${Date.now()}`,
      novel_id: props.id,
      role: "user",
      content: text,
      created_at: now,
    },
    {
      id: pendingId,
      novel_id: props.id,
      role: "assistant",
      content: t("workspace.chatStepAnalyzing"),
      created_at: now,
    },
  ];
  chatInput.value = "";
  await scrollChatBottom();

  const run = startChatRunProgress((lines) => patchPending(pendingId, lines));
  const unlisten = await listen<{ novelId: string; step: string }>("chat-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    const label = chatProgressLabel(ev.payload.step);
    if (!label) return;
    run.advance(ev.payload.step, label);
  });
  const unlistenTokens = await listen<{
    novelId: string;
    promptTokens: number;
    completionTokens: number;
    confirmed: boolean;
  }>("chapter-tokens", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    run.onTokens(ev.payload.promptTokens, ev.payload.completionTokens, ev.payload.confirmed);
  });

  try {
    const list = await api.chatSend(props.id, text, chatModel.value);
    const stats = run.finish();
    messages.value = attachChatRunStats(list, stats);
    novel.value = await api.getNovel(props.id);
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) {
        selected.value = n;
      } else {
        // 如 /清空章节：选中节点已删，清掉预览与记忆面板残留
        selected.value = null;
        chapterMd.value = "";
        memoryItems.value = [];
        memoryPanelOpen.value = false;
        memoryRefPicks.value = [];
        memoryRefSelectedIds.value = [];
      }
    }
    await scrollChatBottom();
  } catch (e) {
    const msg = String(e);
    const stats = run.finish();
    if (msg.toLowerCase().includes("cancelled")) {
      const list = await api.listChat(props.id);
      messages.value = [
        ...list,
        {
          id: `local-stop-${Date.now()}`,
          novel_id: props.id,
          role: "assistant",
          content: appendChatRunStats(t("workspace.chatStopped"), stats),
          created_at: new Date().toISOString(),
        },
      ];
    } else {
      patchPending(pendingId, [
        appendChatRunStats(`${t("workspace.chatStepFailed")}\n${msg}`, stats),
      ]);
      notice.value = msg;
    }
  } finally {
    run.dispose();
    unlisten();
    unlistenTokens();
    busy.value = "";
  }
}

async function stopChat() {
  if (busy.value !== "chat" && busy.value !== "card-chat") return;
  try {
    await api.chatCancel(props.id);
  } catch {
    /* ignore */
  }
}

function emptyCharacter(): NonNullable<TreeNode["character"]> {
  return { role: "", personality: "", motto: "", gender: "", style: "", alignment: "" };
}

function emptyKnowledge(): NonNullable<TreeNode["knowledge"]> {
  return { book_ids: [], extract_prompt: "", extracted: "" };
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

async function autoLayout() {
  if (!tree.value || autoAll.value) return;
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

async function addCard(kind: "chapter" | "character" | "side_plot" | "knowledge") {
  if (!tree.value) return;
  const root = tree.value.nodes.find((n) => n.kind === "novel");
  if (!root) return;
  const id = crypto.randomUUID();
  const maxY = Math.max(...tree.value.nodes.map((n) => n.position.y), 0);
  let node: TreeNode;
  let edge: TreeEdge;

  if (kind === "chapter") {
    const chapters = tree.value.nodes
      .filter((n) => n.kind === "chapter")
      .sort((a, b) => a.position.y - b.position.y);
    const prev = chapters.length ? chapters[chapters.length - 1] : root;
    node = {
      id,
      kind,
      label: t("workspace.newChapter"),
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
    // 默认挂到当前选中卡（根/章/剧情）；否则挂根
    const host =
      selected.value &&
      (selected.value.kind === "novel" ||
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
    root.linked_knowledge_ids = root.linked_knowledge_ids ?? [];
    root.linked_knowledge_ids.push(id);
    edge = {
      id: `e-${root.id}-${id}`,
      source: root.id,
      target: id,
      kind: "knowledge",
      source_handle: "left",
      target_handle: "right",
    };
  } else {
    // 默认挂到当前选中章/根；选中剧情卡时挂到该剧情所挂的章（或根）
    let host = root;
    const sel = selected.value;
    if (sel?.kind === "novel" || sel?.kind === "chapter") {
      host = sel;
    } else if (sel?.kind === "side_plot") {
      const owner = tree.value.nodes.find(
        (n) =>
          (n.kind === "chapter" || n.kind === "novel") &&
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
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
}

function cardMessagesFor(nodeId: string) {
  return cardChats.value[nodeId] ?? [];
}

function setCardMessages(nodeId: string, list: { id: string; role: string; content: string }[]) {
  cardChats.value = { ...cardChats.value, [nodeId]: list };
}

function hasCardChatPending(nodeId: string): boolean {
  return cardMessagesFor(nodeId).some((m) => m.id.startsWith("pending-"));
}

/** force=true 时从 DB 覆盖；否则保留已缓存/进行中的会话。 */
async function loadCardChatHistory(nodeId: string, force = false) {
  if (!force) {
    if (cardChatBusyNodeId === nodeId || hasCardChatPending(nodeId)) return;
    if (cardChatHydrated.value[nodeId]) return;
  }
  try {
    const rows = await api.listCardChat(props.id, nodeId);
    // 请求回来时若已开跑，勿盖掉 pending
    if (!force && (cardChatBusyNodeId === nodeId || hasCardChatPending(nodeId))) return;
    setCardMessages(
      nodeId,
      rows.map((m) => ({ id: m.id, role: m.role, content: m.content })),
    );
    cardChatHydrated.value = { ...cardChatHydrated.value, [nodeId]: true };
  } catch {
    /* keep whatever is in memory */
  }
}

async function sendCardChat() {
  const node = selected.value;
  if (!node || node.kind === "novel") return;
  const text = cardChatInput.value.trim();
  if (!text || busy.value === "card-chat") return;
  busy.value = "card-chat";
  cardChatBusyNodeId = node.id;
  notice.value = "";
  const pendingId = `pending-${Date.now()}`;
  const prev = cardMessagesFor(node.id);
  setCardMessages(node.id, [
    ...prev,
    { id: `u-${Date.now()}`, role: "user", content: text },
    { id: pendingId, role: "assistant", content: t("workspace.chatStepAnalyzing") },
  ]);
  cardChatHydrated.value = { ...cardChatHydrated.value, [node.id]: true };
  cardChatInput.value = "";
  await scrollChatBottom();

  const run = startChatRunProgress((lines) => patchCardPending(node.id, pendingId, lines));
  const unlisten = await listen<{ novelId: string; step: string }>("chat-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    const label = chatProgressLabel(ev.payload.step);
    if (!label) return;
    run.advance(ev.payload.step, label);
  });
  const unlistenChapter = await listen<{
    novelId: string;
    step: string;
    index: number;
    total: number;
  }>("chapter-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    const label = chatProgressLabel(ev.payload.step);
    if (!label) return;
    run.advance(ev.payload.step, label);
  });
  const unlistenTokens = await listen<{
    novelId: string;
    promptTokens: number;
    completionTokens: number;
    confirmed: boolean;
  }>("chapter-tokens", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    run.onTokens(ev.payload.promptTokens, ev.payload.completionTokens, ev.payload.confirmed);
  });

  try {
    const beforeBody = chapterMd.value;
    const r = await api.cardChatSend(props.id, node.id, text, chatModel.value);
    tree.value = r.tree;
    syncFlowFromTree();
    const updated = r.tree.nodes.find((n) => n.id === node.id);
    if (updated) {
      // 用户若已切走卡片，不要强行抢回选中；切回时仍能看到本卡会话
      if (selected.value?.id === node.id) {
        selected.value = updated;
        if (updated.kind === "chapter" || updated.kind === "side_plot") {
          chapterMd.value = await api.getChapter(props.id, updated.id);
          if (
            updated.kind === "chapter" &&
            beforeBody &&
            chapterMd.value &&
            beforeBody !== chapterMd.value
          ) {
            refineBeforeByNode.value = {
              ...refineBeforeByNode.value,
              [updated.id]: beforeBody,
            };
            bodyTab.value = "diff";
          }
        }
      } else if (updated.kind === "chapter" || updated.kind === "side_plot") {
        // 后台完成：正文已变，缓存可稍后进卡再拉
      }
    }
    const stats = run.finish();
    await loadCardChatHistory(node.id, true);
    setCardMessages(node.id, attachChatRunStats(cardMessagesFor(node.id), stats));
    if (selected.value?.id === node.id) await scrollChatBottom();
  } catch (e) {
    const msg = String(e);
    const stats = run.finish();
    const content = appendChatRunStats(
      msg.toLowerCase().includes("cancelled")
        ? t("workspace.chatStopped")
        : `${t("workspace.chatStepFailed")}\n${msg}`,
      stats,
    );
    patchCardPending(node.id, pendingId, [content]);
    if (!msg.toLowerCase().includes("cancelled")) notice.value = msg;
  } finally {
    run.dispose();
    unlisten();
    unlistenChapter();
    unlistenTokens();
    if (cardChatBusyNodeId === node.id) cardChatBusyNodeId = null;
    busy.value = "";
  }
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
const selectedIsNovel = computed(() => selected.value?.kind === "novel");
const canDeleteSelectedCard = computed(() => {
  const k = selected.value?.kind;
  return !!k && k !== "novel";
});

type PendingCardDelete = {
  id: string;
  title: string;
  hasBody: boolean;
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
  if (!tree.value || !selected.value || selected.value.kind === "novel" || autoAll.value) return;
  if (!canDeleteSelectedCard.value || deletingCard.value) return;
  const id = selected.value.id;
  const title = selected.value.label || id;
  const hasBody =
    selected.value.kind === "chapter"
      ? await chapterHasGeneratedBody(id, selected.value.word_count ?? 0)
      : false;
  pendingCardDelete.value = { id, title, hasBody, step: 1 };
}

function cancelPendingCardDelete() {
  if (deletingCard.value) return;
  pendingCardDelete.value = null;
}

async function confirmPendingCardDelete() {
  const p = pendingCardDelete.value;
  if (!p || !tree.value || deletingCard.value) return;
  // 有正文的章节：第二步再确认
  if (p.hasBody && p.step === 1) {
    pendingCardDelete.value = { ...p, step: 2 };
    return;
  }

  deletingCard.value = true;
  const id = p.id;
  try {
    tree.value = await api.deleteTreeCard(props.id, id);
    pruneTreeRefs(tree.value);
    removeNodes([id], true);
    syncFlowFromTree();

    if (cardChats.value[id]) {
      const next = { ...cardChats.value };
      delete next[id];
      cardChats.value = next;
    }
    if (cardChatHydrated.value[id]) {
      const next = { ...cardChatHydrated.value };
      delete next[id];
      cardChatHydrated.value = next;
    }
    if (cardChatBusyNodeId === id) cardChatBusyNodeId = null;
    if (refineBeforeByNode.value[id] !== undefined) {
      const next = { ...refineBeforeByNode.value };
      delete next[id];
      refineBeforeByNode.value = next;
    }

    pendingCardDelete.value = null;
    const root = tree.value.nodes.find((n) => n.kind === "novel");
    if (root) await selectNode(root);
    else selected.value = null;
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
  if (!canDeleteSelectedCard.value || autoAll.value || pendingCardDelete.value) return;
  ev.preventDefault();
  ev.stopPropagation();
  void deleteSelectedCard();
}

const knowledgeDraft = computed(() => selected.value?.knowledge ?? emptyKnowledge());

const filteredKnowledgeBooks = computed(() => {
  const q = knowledgeBookFilter.value.trim().toLowerCase();
  if (!q) return knowledgeBooks.value;
  return knowledgeBooks.value.filter(
    (b) =>
      b.title.toLowerCase().includes(q) ||
      b.genres.some((g) => g.toLowerCase().includes(q)),
  );
});

watch(
  () => selected.value?.id,
  () => {
    knowledgeBookFilter.value = "";
  },
);

function ensureKnowledgePayload() {
  if (!selected.value || selected.value.kind !== "knowledge") return null;
  if (!selected.value.knowledge) selected.value.knowledge = emptyKnowledge();
  return selected.value.knowledge;
}

function toggleKnowledgeBook(bookId: string) {
  const k = ensureKnowledgePayload();
  if (!k) return;
  if (k.book_ids.includes(bookId)) {
    k.book_ids = k.book_ids.filter((id) => id !== bookId);
  } else {
    k.book_ids = [...k.book_ids, bookId];
  }
  void persistSelectedKnowledge();
}

async function persistSelectedKnowledge() {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
  if (n) {
    n.knowledge = selected.value.knowledge;
    n.label = selected.value.label;
    // Tree preview mirrors extracted features (same as AI extract).
    const feat = selected.value.knowledge?.extracted?.trim() ?? "";
    n.outline = feat ? [...feat].slice(0, 200).join("") : "";
  }
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
}

async function persistSelectedCardText() {
  const sel = selected.value;
  if (!tree.value || !sel) return;
  if (sel.kind !== "novel" && sel.kind !== "chapter" && sel.kind !== "side_plot") return;
  const label = sel.label.trim();
  if ((sel.kind === "chapter" || sel.kind === "side_plot") && !label) return;
  const n = tree.value.nodes.find((x) => x.id === sel.id);
  if (!n) return;
  if (sel.kind === "chapter" || sel.kind === "side_plot") {
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

const pendingConsolidate = ref<{
  chapterWindow: number;
  userNotes: string;
} | null>(null);

function openConsolidatePlots() {
  if (!selectedIsNovel.value || !!busy.value || autoAll.value) return;
  pendingConsolidate.value = { chapterWindow: 5, userNotes: "" };
}

function cancelConsolidatePlots() {
  pendingConsolidate.value = null;
}

async function confirmConsolidatePlots() {
  const p = pendingConsolidate.value;
  if (!p) return;
  const windowN = Math.max(1, Math.min(30, Number(p.chapterWindow) || 5));
  const notes = p.userNotes;
  pendingConsolidate.value = null;
  busy.value = "consolidate-plots";
  beginChapterTask("context", 1, 3);
  chapterResultNotice.value = "";
  try {
    const r = await api.consolidatePlotCards(props.id, windowN, notes);
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    chapterResultNotice.value = r.message;
    await nextTick();
    void fitView({ padding: 0.18, duration: 280 });
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
  } finally {
    endChapterTask();
    busy.value = "";
  }
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

const canonBusy = ref(false);

function toggleCanonBook(id: string) {
  if (!novel.value) return;
  const cur = [...(novel.value.knowledge_ids ?? [])];
  const i = cur.indexOf(id);
  if (i >= 0) cur.splice(i, 1);
  else cur.push(id);
  novel.value.knowledge_ids = cur;
  void persistNovelCanon();
}

async function persistNovelCanon() {
  if (!novel.value) return;
  try {
    novel.value = await api.updateNovelCanon(
      props.id,
      novel.value.knowledge_ids ?? [],
      novel.value.canon_mode || "reference",
      novel.value.knowledge_strategy ?? "",
    );
  } catch (e) {
    notice.value = String(e);
  }
}

async function syncCanonSettings() {
  if (!novel.value || !!busy.value || autoAll.value || canonBusy.value) return;
  if (!(novel.value.knowledge_ids ?? []).length) {
    notice.value = t("workspace.canonNeedBooks");
    return;
  }
  canonBusy.value = true;
  busy.value = "sync-canon";
  beginChapterTask("context", 1, Math.max(1, novel.value.knowledge_ids.length));
  chapterResultNotice.value = "";
  try {
    const r = await api.syncCanonSettings(props.id);
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value?.kind === "novel") {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
  } finally {
    endChapterTask();
    busy.value = "";
    canonBusy.value = false;
  }
}

async function runExtractKnowledge() {
  if (!selected.value || selected.value.kind !== "knowledge") return;
  await persistSelectedKnowledge();
  knowledgeExtractBusy.value = true;
  notice.value = "";
  try {
    tree.value = await extractKnowledgeCard(props.id, selected.value.id);
    const updated = tree.value.nodes.find((n) => n.id === selected.value!.id);
    if (updated) selected.value = updated;
    syncFlowFromTree();
    notice.value = t("workspace.knowledgeExtracted");
  } catch (e) {
    notice.value = String(e);
  } finally {
    knowledgeExtractBusy.value = false;
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

const chapterHtml = computed(() => (chapterMd.value ? renderChapterMd(chapterMd.value) : ""));

/** 本章剧情/人物 tag：含根节点贯穿人物、剧情卡上挂的人物 */
const chapterLinkTags = computed(() => {
  const ch = selected.value;
  const tr = tree.value;
  if (!ch || ch.kind !== "chapter" || !tr) return { plots: [] as string[], chars: [] as string[] };

  const charIds = new Set<string>(ch.linked_character_ids);
  const plotIds = new Set<string>(ch.linked_side_plot_ids);

  for (const e of tr.edges) {
    if (e.source !== ch.id && e.target !== ch.id) continue;
    const otherId = e.source === ch.id ? e.target : e.source;
    const other = tr.nodes.find((n) => n.id === otherId);
    if (!other) continue;
    if (other.kind === "character") charIds.add(otherId);
    if (other.kind === "side_plot") plotIds.add(otherId);
  }

  const root = tr.nodes.find((n) => n.kind === "novel");
  if (root) {
    for (const cid of root.linked_character_ids) charIds.add(cid);
    for (const e of tr.edges) {
      if (e.source !== root.id && e.target !== root.id) continue;
      const otherId = e.source === root.id ? e.target : e.source;
      if (tr.nodes.find((n) => n.id === otherId)?.kind === "character") charIds.add(otherId);
    }
  }

  for (const pid of [...plotIds]) {
    const plot = tr.nodes.find((n) => n.id === pid);
    if (plot) for (const cid of plot.linked_character_ids) charIds.add(cid);
    for (const e of tr.edges) {
      if (e.source !== pid && e.target !== pid) continue;
      const otherId = e.source === pid ? e.target : e.source;
      if (tr.nodes.find((n) => n.id === otherId)?.kind === "character") charIds.add(otherId);
    }
  }

  const plots = [...plotIds]
    .map((id) => tr.nodes.find((n) => n.id === id)?.label)
    .filter((x): x is string => !!x);
  const chars = [...charIds]
    .map((id) => tr.nodes.find((n) => n.id === id)?.label)
    .filter((x): x is string => !!x);
  return { plots, chars };
});

const chapterBusy = computed(
  () =>
    busy.value === "generate" ||
    busy.value === "refine" ||
    busy.value === "plan-next" ||
    busy.value === "gen-plots" ||
    busy.value === "regen-memory" ||
    busy.value === "gen-cards" ||
    busy.value === "sync-canon" ||
    busy.value === "consolidate-plots",
);
const cardChatMode = computed(() => {
  const k = selected.value?.kind;
  return k === "chapter" || k === "character" || k === "side_plot";
});
const activeCardMessages = computed(() =>
  selected.value ? cardMessagesFor(selected.value.id) : [],
);
const cardChatTitle = computed(() => {
  const n = selected.value;
  if (!n) return t("workspace.chat");
  if (n.kind === "chapter" || n.kind === "character" || n.kind === "side_plot") {
    return `${n.label} Chat`;
  }
  return t("workspace.chat");
});
const cardChatPlaceholder = computed(() => {
  const k = selected.value?.kind;
  if (k === "chapter") return t("workspace.cardChatChapterPh");
  if (k === "character") return t("workspace.cardChatCharacterPh");
  if (k === "side_plot") return t("workspace.cardChatPlotPh");
  return t("workspace.chatPlaceholder");
});

type SlashCmd = { cmd: string; insert: string; hint: string };

const chatSkills = ref<SkillPreviewItem[]>([]);

const rootSlashCmds = computed<SlashCmd[]>(() => {
  const zh = locale.value.startsWith("zh") || locale.value === "ja";
  if (zh) {
    return [
      { cmd: "/章节卡", insert: "/章节卡 ", hint: t("workspace.slashHintChapters") },
      { cmd: "/刷新大纲", insert: "/刷新大纲 ", hint: t("workspace.slashHintRegenOutline") },
      { cmd: "/添加剧情", insert: "/添加剧情 ", hint: t("workspace.slashHintAddPlot") },
      { cmd: "/剧情卡", insert: "/剧情卡 ", hint: t("workspace.slashHintPlots") },
      { cmd: "/清空章节", insert: "/清空章节", hint: t("workspace.slashHintClear") },
      { cmd: "/清空所有剧情", insert: "/清空所有剧情", hint: t("workspace.slashHintClearPlots") },
    ];
  }
  return [
    { cmd: "/chapters", insert: "/chapters ", hint: t("workspace.slashHintChapters") },
    { cmd: "/refresh-outline", insert: "/refresh-outline ", hint: t("workspace.slashHintRegenOutline") },
    { cmd: "/add-plot", insert: "/add-plot ", hint: t("workspace.slashHintAddPlot") },
    { cmd: "/plots", insert: "/plots ", hint: t("workspace.slashHintPlots") },
    { cmd: "/clear-chapters", insert: "/clear-chapters", hint: t("workspace.slashHintClear") },
    { cmd: "/clear-plots", insert: "/clear-plots", hint: t("workspace.slashHintClearPlots") },
  ];
});

const chapterSlashCmds = computed<SlashCmd[]>(() => {
  const zh = locale.value.startsWith("zh") || locale.value === "ja";
  if (zh) {
    return [
      { cmd: "/完善剧情", insert: "/完善剧情", hint: t("workspace.slashHintEnrichPlots") },
      {
        cmd: "/刷新本章大纲",
        insert: "/刷新本章大纲",
        hint: t("workspace.slashHintRegenOutlineChapter"),
      },
      { cmd: "/预生成", insert: "/预生成", hint: t("workspace.slashHintGenerate") },
      { cmd: "/精修", insert: "/精修", hint: t("workspace.slashHintRefine") },
    ];
  }
  return [
    { cmd: "/enrich-plots", insert: "/enrich-plots", hint: t("workspace.slashHintEnrichPlots") },
    {
      cmd: "/refresh-this-outline",
      insert: "/refresh-this-outline",
      hint: t("workspace.slashHintRegenOutlineChapter"),
    },
    { cmd: "/generate", insert: "/generate", hint: t("workspace.slashHintGenerate") },
    { cmd: "/refine", insert: "/refine", hint: t("workspace.slashHintRefine") },
  ];
});

const skillSlashCmds = computed<SlashCmd[]>(() =>
  chatSkills.value.map((s) => {
    const line = (s.description || "").split(/\n/)[0]?.trim() || t("workspace.slashHintSkill");
    const hint = line.length > 72 ? `${line.slice(0, 72)}…` : line;
    return {
      cmd: `/${s.name}`,
      insert: `/${s.name} `,
      hint,
    };
  }),
);

const slashActive = ref(0);

/** 输入以 / 开头且尚未空格时，提示可补全指令（含 skills） */
const slashSuggestions = computed(() => {
  const chapterCard = selected.value?.kind === "chapter";
  const builtIn = chapterCard
    ? chapterSlashCmds.value
    : cardChatMode.value
      ? []
      : rootSlashCmds.value;
  const cmds = [...builtIn, ...skillSlashCmds.value];
  if (!cmds.length) return [];
  const v = chapterCard || cardChatMode.value ? cardChatInput.value : chatInput.value;
  if (!v.startsWith("/") || /\s/.test(v)) return [];
  const q = v.toLowerCase();
  return cmds.filter((c) => c.cmd.toLowerCase().startsWith(q) || c.cmd.startsWith(v));
});

watch(slashSuggestions, () => {
  slashActive.value = 0;
});

function applySlashCmd(cmd: SlashCmd) {
  if (cardChatMode.value) cardChatInput.value = cmd.insert;
  else chatInput.value = cmd.insert;
  slashActive.value = 0;
}

function onChatKeydown(ev: KeyboardEvent) {
  const list = slashSuggestions.value;
  if (list.length) {
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      slashActive.value = (slashActive.value + 1) % list.length;
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      slashActive.value = (slashActive.value - 1 + list.length) % list.length;
    } else if (ev.key === "Enter" || ev.key === "Tab") {
      ev.preventDefault();
      applySlashCmd(list[slashActive.value] ?? list[0]);
    } else if (ev.key === "Escape") {
      ev.preventDefault();
      if (cardChatMode.value) cardChatInput.value = "";
      else chatInput.value = "";
    }
    return;
  }
  // Enter 发送；Shift+Enter 换行
  if (ev.key === "Enter" && !ev.shiftKey) {
    ev.preventDefault();
    if (cardChatMode.value) void sendCardChat();
    else void sendChat();
  }
}

watch(
  () => selected.value?.id,
  () => {
    cardChatInput.value = "";
  },
);

/** 三区尺寸比例跨会话记住 */
const leftW = useLocalStorage("novework.workspaceLeftW", 380);
const rightW = useLocalStorage("novework.workspaceRightW", 320);
/** Chat 停靠：右侧栏 | 树图下方 */
const chatDock = useLocalStorage<"right" | "bottom">("novework.chatDock", "right");
const chatBottomH = useLocalStorage("novework.chatBottomH", 280);
const MIN_SIDE = 220;
const MIN_MID = 280;
const MIN_CHAT_H = 160;

const workspaceGridStyle = computed(() => {
  // 左栏（章节编辑）始终满高独立；树图与 Chat 只在右侧区域切换
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
      <!-- 左：章节预览 -->
      <div class="flex min-h-0 min-w-0 flex-col border-r" style="grid-area: left">
        <div class="shrink-0 space-y-2 border-b p-3">
          <div class="flex items-start justify-between gap-2">
            <div
              v-if="selectedIsChapter || selected?.kind === 'side_plot'"
              class="min-w-0 flex-1 space-y-1"
            >
              <label class="block text-[11px] text-muted-foreground">{{
                selectedIsChapter ? t("workspace.chapterTitle") : t("workspace.plotTitle")
              }}</label>
              <Input
                class="h-8 text-sm font-medium"
                :model-value="selected?.label ?? ''"
                :placeholder="
                  selectedIsChapter ? t('workspace.chapterTitlePh') : t('workspace.plotTitlePh')
                "
                :disabled="autoAll || chapterBusy"
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
              :disabled="autoAll"
              @click="deleteSelectedCard"
            >
              {{ t("workspace.deleteCard") }}
            </Button>
          </div>
          <div
            v-if="selectedIsChapter || selected?.kind === 'side_plot'"
            class="space-y-1"
          >
            <label class="block text-[11px] text-muted-foreground">{{
              selectedIsChapter ? t("workspace.chapterOutline") : t("workspace.plotOutline")
            }}</label>
            <Textarea
              :model-value="selected?.outline ?? ''"
              rows="4"
              class="max-h-40 text-xs"
              :placeholder="
                selectedIsChapter ? t('workspace.chapterOutlinePh') : t('workspace.plotOutlinePh')
              "
              :disabled="autoAll || chapterBusy"
              @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
              @change="persistSelectedCardText"
            />
            <p class="text-[10px] text-muted-foreground">{{
              selectedIsChapter ? t("workspace.chapterOutlineHint") : t("workspace.plotOutlineHint")
            }}</p>
            <div v-if="selected?.kind === 'side_plot'" class="space-y-1.5 pt-1">
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
                  :disabled="autoAll || chapterBusy"
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
                :disabled="autoAll || chapterBusy"
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
          </div>
          <div
            v-if="selectedIsChapter && (chapterLinkTags.plots.length || chapterLinkTags.chars.length)"
            class="flex flex-wrap gap-1"
          >
            <span
              v-for="name in chapterLinkTags.plots"
              :key="'p-' + name"
              class="rounded bg-sky-100 px-1.5 py-0.5 text-[10px] text-sky-900"
              :title="t('workspace.plotCard')"
            >{{ name }}</span>
            <span
              v-for="name in chapterLinkTags.chars"
              :key="'c-' + name"
              class="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] text-amber-900"
              :title="t('workspace.role')"
            >{{ name }}</span>
          </div>
          <div v-if="selectedIsChapter" class="space-y-2">
            <!-- 辅助：停止 / 复制 / 记忆（预生成/精修/剧情卡/刷新本章大纲 → 右侧 Chat / 指令） -->
            <div class="flex flex-wrap items-center gap-2">
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
                :disabled="!!busy || autoAll || bodyEditing"
                :title="t('workspace.editBody')"
                :aria-label="t('workspace.editBody')"
                @click="startEditChapterBody"
              >
                <Pencil class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="px-2"
                :disabled="!!busy || autoAll || !chapterMd || bodyEditing"
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
                :disabled="memoryBusy || autoAll || (chapterBusy && busy !== 'regen-memory') || bodyEditing"
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
            </div>
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
        <div class="relative min-h-0 flex-1 overflow-auto p-4">
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
          />
          <div v-else-if="selectedIsKnowledge" class="space-y-4 text-sm">
            <div>
              <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.knowledgeLabel") }}</label>
              <Input
                :model-value="selected?.label ?? ''"
                class="h-8"
                @update:model-value="(v) => { if (selected) selected.label = String(v); }"
                @change="persistSelectedKnowledge"
              />
            </div>
            <div>
              <div class="mb-1 flex items-center justify-between gap-2">
                <label class="text-xs text-muted-foreground">{{ t("workspace.knowledgePickBooks") }}</label>
                <span v-if="knowledgeBooks.length" class="shrink-0 text-[10px] text-muted-foreground">
                  {{ t("workspace.knowledgeSelectedCount", { n: knowledgeDraft.book_ids.length }) }}
                </span>
              </div>
              <p v-if="!knowledgeBooks.length" class="text-xs text-muted-foreground">{{ t("workspace.knowledgeNoBooks") }}</p>
              <div v-else class="space-y-1.5">
                <Input
                  v-model="knowledgeBookFilter"
                  class="h-7 text-xs"
                  :placeholder="t('workspace.knowledgeSearchBooks')"
                />
                <div class="max-h-44 overflow-y-auto rounded-md border">
                  <button
                    v-for="b in filteredKnowledgeBooks"
                    :key="b.id"
                    type="button"
                    class="flex w-full items-center gap-2 border-b border-border/50 px-2 py-1.5 text-left last:border-b-0 hover:bg-muted/50"
                    :class="knowledgeDraft.book_ids.includes(b.id) ? 'bg-teal-50/90' : ''"
                    :title="[b.title, ...b.genres].filter(Boolean).join(' · ')"
                    @click="toggleKnowledgeBook(b.id)"
                  >
                    <span
                      class="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-sm border text-[9px] leading-none"
                      :class="
                        knowledgeDraft.book_ids.includes(b.id)
                          ? 'border-teal-700 bg-teal-700 text-white'
                          : 'border-muted-foreground/40'
                      "
                      aria-hidden="true"
                    >
                      <span v-if="knowledgeDraft.book_ids.includes(b.id)">✓</span>
                    </span>
                    <span class="min-w-0 flex-1 truncate text-xs">
                      {{ b.title }}
                      <span v-if="b.genres.length" class="text-muted-foreground">
                        · {{ b.genres.join("、") }}
                      </span>
                    </span>
                  </button>
                  <p
                    v-if="!filteredKnowledgeBooks.length"
                    class="px-2 py-3 text-center text-xs text-muted-foreground"
                  >
                    {{ t("workspace.knowledgeSearchEmpty") }}
                  </p>
                </div>
              </div>
            </div>
            <div>
              <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.knowledgeExtractPrompt") }}</label>
              <Textarea
                :model-value="knowledgeDraft.extract_prompt"
                rows="4"
                :placeholder="t('workspace.knowledgeExtractPh')"
                @update:model-value="(v) => { const k = ensureKnowledgePayload(); if (k) k.extract_prompt = String(v); }"
                @change="persistSelectedKnowledge"
              />
            </div>
            <Button
              size="sm"
              :disabled="knowledgeExtractBusy || !knowledgeDraft.book_ids.length || !knowledgeDraft.extract_prompt.trim()"
              @click="runExtractKnowledge"
            >
              <Loader2 v-if="knowledgeExtractBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              {{ knowledgeExtractBusy ? t("workspace.knowledgeExtracting") : t("workspace.knowledgeExtract") }}
            </Button>
            <div class="space-y-1">
              <label class="block text-xs text-muted-foreground">{{ t("workspace.knowledgeFeatures") }}</label>
              <Textarea
                :model-value="knowledgeDraft.extracted"
                rows="10"
                class="min-h-[10rem] text-sm"
                :placeholder="t('workspace.knowledgeFeaturesPh')"
                :disabled="knowledgeExtractBusy"
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
          <template v-else-if="selectedIsChapter && bodyEditing">
            <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
              <p class="text-xs text-muted-foreground">{{ t("workspace.editBodyHint") }}</p>
              <div class="flex gap-2">
                <Button size="sm" variant="outline" :disabled="bodySaveBusy" @click="cancelEditChapterBody">
                  {{ t("novels.cancel") }}
                </Button>
                <Button size="sm" :disabled="bodySaveBusy" @click="saveChapterBody">
                  <Loader2 v-if="bodySaveBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  {{ bodySaveBusy ? t("workspace.bodySaving") : t("workspace.saveBody") }}
                </Button>
              </div>
            </div>
            <Textarea
              v-model="bodyDraft"
              rows="24"
              class="min-h-[min(60vh,28rem)] font-sans text-sm leading-relaxed"
              :disabled="bodySaveBusy"
              :placeholder="t('workspace.editBodyPh')"
              :aria-label="t('workspace.editBody')"
            />
          </template>
          <template v-else-if="chapterMd">
            <div
              v-if="selectedIsChapter && hasRefineDiff"
              class="mb-3 flex flex-wrap items-center gap-1 border-b pb-2"
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
              v-if="bodyTab === 'body' || !hasRefineDiff"
              ref="chapterBodyEl"
              class="chapter-md text-sm leading-relaxed"
              :class="chapterBusy ? 'opacity-40' : ''"
              v-html="chapterHtml"
            />
            <div
              v-else
              class="space-y-0.5 font-sans text-sm leading-relaxed"
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
          </template>
          <div v-else-if="selectedIsNovel" class="space-y-4">
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
              <p class="text-xs font-medium">{{ t("workspace.consolidatePlots") }}</p>
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.consolidatePlotsHint") }}</p>
              <Button
                size="sm"
                variant="outline"
                class="h-8"
                :disabled="!!busy || autoAll"
                @click="openConsolidatePlots"
              >
                <Loader2
                  v-if="busy === 'consolidate-plots'"
                  class="mr-1.5 h-3.5 w-3.5 animate-spin"
                />
                <GitBranch v-else class="mr-1.5 h-3.5 w-3.5" />
                {{
                  busy === "consolidate-plots"
                    ? t("workspace.consolidatePlotsBusy")
                    : t("workspace.consolidatePlots")
                }}
              </Button>
            </div>
            <div class="space-y-2 rounded-md border bg-muted/30 p-3">
              <p class="text-xs font-medium">{{ t("workspace.canonTitle") }}</p>
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.canonHint") }}</p>
              <div class="flex flex-wrap gap-1.5">
                <button
                  type="button"
                  class="rounded px-2 py-1 text-[11px] transition-colors"
                  :class="
                    (novel?.canon_mode || 'reference') === 'strict'
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:bg-muted'
                  "
                  :disabled="!!busy || autoAll"
                  @click="
                    () => {
                      if (novel) novel.canon_mode = 'strict';
                      void persistNovelCanon();
                    }
                  "
                >
                  {{ t("workspace.canonModeStrict") }}
                </button>
                <button
                  type="button"
                  class="rounded px-2 py-1 text-[11px] transition-colors"
                  :class="
                    (novel?.canon_mode || 'reference') !== 'strict'
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:bg-muted'
                  "
                  :disabled="!!busy || autoAll"
                  @click="
                    () => {
                      if (novel) novel.canon_mode = 'reference';
                      void persistNovelCanon();
                    }
                  "
                >
                  {{ t("workspace.canonModeReference") }}
                </button>
              </div>
              <div class="max-h-36 space-y-1 overflow-y-auto rounded-md border bg-background/80 p-2">
                <p v-if="!knowledgeBooks.length" class="text-[11px] text-muted-foreground">
                  {{ t("workspace.knowledgeNoBooks") }}
                </p>
                <button
                  v-for="b in knowledgeBooks"
                  :key="b.id"
                  type="button"
                  class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-muted"
                  :disabled="!!busy || autoAll"
                  @click="toggleCanonBook(b.id)"
                >
                  <span
                    class="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded border text-[9px]"
                    :class="
                      (novel?.knowledge_ids ?? []).includes(b.id)
                        ? 'border-primary bg-primary text-primary-foreground'
                        : 'border-border'
                    "
                  >
                    {{ (novel?.knowledge_ids ?? []).includes(b.id) ? "✓" : "" }}
                  </span>
                  <span class="min-w-0 truncate">{{ b.title }}</span>
                </button>
              </div>
              <div>
                <label class="mb-1 block text-[11px] text-muted-foreground">{{
                  t("workspace.canonStrategy")
                }}</label>
                <Textarea
                  :model-value="novel?.knowledge_strategy ?? ''"
                  rows="2"
                  class="text-xs"
                  :placeholder="t('workspace.canonStrategyPh')"
                  :disabled="!!busy || autoAll"
                  @update:model-value="(v) => { if (novel) novel.knowledge_strategy = String(v); }"
                  @change="persistNovelCanon"
                />
              </div>
              <Button
                size="sm"
                variant="outline"
                class="h-8"
                :disabled="!!busy || autoAll || !(novel?.knowledge_ids ?? []).length"
                :title="t('workspace.canonSyncHint')"
                @click="syncCanonSettings"
              >
                <Loader2 v-if="busy === 'sync-canon'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                {{
                  busy === "sync-canon" ? t("workspace.canonSyncBusy") : t("workspace.canonSync")
                }}
              </Button>
            </div>
            <div class="space-y-2 rounded-md border bg-muted/30 p-3">
              <p class="text-xs font-medium">{{ t("workspace.rootGenChapters") }}</p>
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.rootGenChaptersHint") }}</p>
              <div class="flex flex-wrap items-center gap-2">
                <div class="flex items-center gap-1 text-[11px] text-muted-foreground">
                  <Input
                    v-model.number="rootGenChapterCount"
                    type="number"
                    min="1"
                    :max="Math.min(100, Math.max(1, novel?.chapter_count || 100))"
                    class="h-8 w-16 px-1.5 text-center text-xs"
                    :disabled="!!busy || autoAll"
                    :aria-label="t('workspace.rootGenChaptersCount')"
                  />
                  <span>{{ t("workspace.chapters") }}</span>
                </div>
                <Button
                  size="sm"
                  class="h-8"
                  :disabled="!!busy || autoAll"
                  @click="openRootGenBrief"
                >
                  <Loader2 v-if="busy === 'gen-cards'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  {{
                    busy === "gen-cards"
                      ? t("workspace.rootGenChaptersBusy")
                      : t("workspace.rootGenChapters")
                  }}
                </Button>
                <Button
                  v-if="busy === 'gen-cards'"
                  size="sm"
                  variant="destructive"
                  class="h-8"
                  @click="stopChapter"
                >
                  <Square class="mr-1.5 h-3.5 w-3.5" />
                  {{ t("workspace.stop") }}
                </Button>
              </div>
            </div>
            <div class="space-y-2">
              <label class="block text-xs text-muted-foreground">{{ t("workspace.rootOutline") }}</label>
              <Textarea
                :model-value="selected?.outline ?? ''"
                rows="12"
                class="min-h-[10rem] text-sm"
                :placeholder="t('workspace.rootOutlinePh')"
                @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                @change="persistSelectedCardText"
              />
              <p class="text-xs text-muted-foreground">{{ t("workspace.rootOutlineHint") }}</p>
            </div>
          </div>
          <div v-else class="text-sm text-muted-foreground">
            <template v-if="selectedIsChapter">
              <p>{{ t("workspace.noBody") }}</p>
              <Button
                size="sm"
                variant="outline"
                class="mt-3"
                :disabled="!!busy || autoAll"
                @click="startEditChapterBody"
              >
                <Pencil class="mr-1.5 h-3.5 w-3.5" />
                {{ t("workspace.editBody") }}
              </Button>
            </template>
            <template v-else-if="selected?.kind === 'side_plot'">{{ t("workspace.sidePlotHint") }}</template>
            <template v-else>{{ t("workspace.clickNode") }}</template>
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
        <div class="absolute left-2 top-2 z-10 flex flex-wrap items-center gap-1.5">
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
              :disabled="autoAll"
              :title="t('workspace.addChapter')"
              :aria-label="t('workspace.addChapter')"
              @click="addCard('chapter')"
            >
              <BookOpen class="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="ghost"
              class="h-7 w-7 px-0 text-amber-800 hover:bg-amber-100 hover:text-amber-900"
              :disabled="autoAll"
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
              :disabled="autoAll"
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
              :disabled="autoAll"
              :title="t('workspace.addKnowledge')"
              :aria-label="t('workspace.addKnowledge')"
              @click="addCard('knowledge')"
            >
              <Library class="h-3.5 w-3.5" />
            </Button>
          </div>
          <Button
            size="sm"
            variant="outline"
            class="h-7 w-7 bg-background/95 px-0"
            :disabled="autoAll || !tree"
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
            :disabled="autoAll || !tree || allMemoryBusy"
            :title="t('workspace.allChapterMemory')"
            :aria-label="t('workspace.allChapterMemory')"
            @click="openAllChapterMemory"
          >
            <Loader2 v-if="allMemoryBusy" class="h-3.5 w-3.5 animate-spin" />
            <Brain v-else class="h-3.5 w-3.5" />
          </Button>
        </div>
        <div class="absolute right-2 top-2 z-10">
          <Button
            size="sm"
            class="h-8 shadow-md"
            :variant="autoAll ? 'destructive' : 'default'"
            :disabled="!!busy && !autoAll"
            :title="autoAll ? t('workspace.autoAllStop') : t('workspace.autoAll')"
            @click="autoAll ? stopAutoAll() : runAutoAll()"
          >
            <Loader2 v-if="autoAll && chapterBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            <Square v-else-if="autoAll" class="mr-1.5 h-3.5 w-3.5" />
            <Play v-else class="mr-1.5 h-3.5 w-3.5" />
            {{ autoAll ? t("workspace.autoAllStop") : t("workspace.autoAll") }}
          </Button>
        </div>
        <div class="pointer-events-none absolute left-2 top-11 z-10 rounded bg-background/90 px-2 py-1 text-[10px] text-muted-foreground shadow">
          {{ t("workspace.flowHint") }}
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

      <!-- Chat：右侧栏 或 树图下方（宽=左栏+树图） -->
      <div
        class="flex min-h-0 min-w-0 flex-col bg-background"
        style="grid-area: chat"
        :class="chatDock === 'right' ? 'border-l' : 'border-t'"
      >
        <div class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2">
          <div class="min-w-0 truncate text-sm font-medium">
            {{ cardChatMode ? cardChatTitle : t("workspace.chat") }}
          </div>
          <div
            class="inline-flex shrink-0 rounded-md border bg-muted/40 p-0.5"
            role="group"
            :aria-label="t('workspace.chatDockHint')"
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
              :title="t('workspace.chatDockRight')"
              :aria-label="t('workspace.chatDockRight')"
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
              :title="t('workspace.chatDockBottom')"
              :aria-label="t('workspace.chatDockBottom')"
              @click="chatDock = 'bottom'"
            >
              <PanelBottom class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
        <div ref="chatListEl" class="min-h-0 flex-1 space-y-3 overflow-auto p-3">
          <template v-if="cardChatMode">
            <p v-if="!activeCardMessages.length" class="text-xs text-muted-foreground">
              {{ t("workspace.cardChatHint") }}
            </p>
            <div
              v-for="m in activeCardMessages"
              :key="m.id"
              class="rounded-lg px-3 py-2 text-sm"
              :class="m.role === 'user' ? 'ml-6 bg-accent' : 'mr-4 bg-muted'"
            >
              <div class="mb-1 text-[10px] uppercase text-muted-foreground">{{ m.role }}</div>
              <div
                class="whitespace-pre-wrap break-words leading-relaxed"
                :class="m.id.startsWith('pending-') ? 'italic text-muted-foreground' : ''"
              >
                {{ formatChatContent(m.content) }}
              </div>
            </div>
          </template>
          <template v-else>
            <div
              v-for="m in messages"
              :key="m.id"
              class="rounded-lg px-3 py-2 text-sm"
              :class="m.role === 'user' ? 'ml-6 bg-accent' : 'mr-4 bg-muted'"
            >
              <div class="mb-1 text-[10px] uppercase text-muted-foreground">{{ m.role }}</div>
              <div
                class="whitespace-pre-wrap break-words leading-relaxed"
                :class="m.id.startsWith('pending-') ? 'italic text-muted-foreground' : ''"
              >
                {{ formatChatContent(m.content) }}
              </div>
            </div>
          </template>
        </div>
        <Separator />
        <div class="space-y-2 p-3">
          <p
            v-if="!cardChatMode"
            class="text-[11px] leading-relaxed text-muted-foreground"
          >
            {{ t("workspace.chatCommands") }}
          </p>
          <p
            v-else-if="selectedIsChapter"
            class="text-[11px] leading-relaxed text-muted-foreground"
          >
            {{ t("workspace.cardChatHint") }}
          </p>
          <div v-if="cardChatMode" class="relative">
            <ul
              v-if="slashSuggestions.length"
              class="absolute bottom-full left-0 right-0 z-20 mb-1 max-h-64 overflow-auto rounded-md border bg-popover py-1 text-sm shadow-md"
              role="listbox"
            >
              <li
                v-for="(s, i) in slashSuggestions"
                :key="s.cmd"
                class="cursor-pointer px-3 py-1.5"
                :class="i === slashActive ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'"
                role="option"
                :aria-selected="i === slashActive"
                @mousedown.prevent="applySlashCmd(s)"
              >
                <span class="font-medium">{{ s.cmd }}</span>
                <span class="ml-2 text-xs text-muted-foreground">{{ s.hint }}</span>
              </li>
            </ul>
            <Textarea
              v-model="cardChatInput"
              rows="3"
              :placeholder="cardChatPlaceholder"
              @keydown="onChatKeydown"
            />
          </div>
          <div v-else class="relative">
            <ul
              v-if="slashSuggestions.length"
              class="absolute bottom-full left-0 right-0 z-20 mb-1 max-h-64 overflow-auto rounded-md border bg-popover py-1 text-sm shadow-md"
              role="listbox"
            >
              <li
                v-for="(s, i) in slashSuggestions"
                :key="s.cmd"
                class="cursor-pointer px-3 py-1.5"
                :class="i === slashActive ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'"
                role="option"
                :aria-selected="i === slashActive"
                @mousedown.prevent="applySlashCmd(s)"
              >
                <span class="font-medium">{{ s.cmd }}</span>
                <span class="ml-2 text-xs text-muted-foreground">{{ s.hint }}</span>
              </li>
            </ul>
            <Textarea
              v-model="chatInput"
              rows="3"
              :placeholder="t('workspace.chatPlaceholder')"
              @keydown="onChatKeydown"
            />
          </div>
          <div class="flex gap-2">
            <select
              v-model="chatModel"
              class="h-9 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-xs"
              :aria-label="t('chat.pickModel')"
              :disabled="busy === 'chat' || busy === 'card-chat'"
              @change="persistChatModel"
            >
              <option v-for="m in chatModelOptions" :key="m.id" :value="m.id">{{ m.label }}</option>
            </select>
            <Button
              v-if="busy === 'chat' || busy === 'card-chat'"
              class="shrink-0"
              variant="destructive"
              @click="stopChat"
            >
              <Square class="mr-1.5 h-3.5 w-3.5" />
              {{ t("workspace.stop") }}
            </Button>
            <Button
              v-else
              class="shrink-0"
              @click="cardChatMode ? sendCardChat() : sendChat()"
            >
              {{ t("workspace.send") }}
            </Button>
          </div>
        </div>
      </div>
    </div>

    <!-- 根节点：整理跨章剧情 -->
    <div
      v-if="pendingConsolidate"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelConsolidatePlots"
    >
      <div
        class="flex w-full max-w-lg flex-col rounded-lg border bg-background p-5 shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <h2 class="text-base font-semibold">{{ t("workspace.consolidatePlotsTitle") }}</h2>
        <p class="mt-2 text-xs text-muted-foreground">{{ t("workspace.consolidatePlotsBody") }}</p>
        <label class="mt-3 block text-xs font-medium text-muted-foreground">
          {{ t("workspace.consolidatePlotsWindow") }}
        </label>
        <Input
          v-model.number="pendingConsolidate.chapterWindow"
          type="number"
          min="1"
          max="30"
          class="mt-1 h-8 w-24"
        />
        <label class="mt-3 block text-xs font-medium text-muted-foreground">
          {{ t("workspace.consolidatePlotsNotes") }}
        </label>
        <Textarea
          v-model="pendingConsolidate.userNotes"
          rows="4"
          class="mt-1 text-sm"
          :placeholder="t('workspace.consolidatePlotsNotesPh')"
        />
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" @click="cancelConsolidatePlots">{{ t("novels.cancel") }}</Button>
          <Button @click="confirmConsolidatePlots">{{ t("workspace.consolidatePlotsConfirm") }}</Button>
        </div>
      </div>
    </div>

    <!-- 生成章节卡 / 下一章：确认须考虑材料与期望 -->
    <div
      v-if="pendingOutlineGen"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelOutlineGen"
    >
      <div
        class="flex w-full max-w-2xl max-h-[min(90vh,720px)] flex-col rounded-lg border bg-background p-5 shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <h2 class="text-base font-semibold">{{ t("workspace.outlineGenTitleRoot") }}</h2>
        <p class="mt-2 text-xs text-muted-foreground">
          {{
            pendingOutlineGen.loading
              ? t("workspace.outlineGenLoading")
              : t("workspace.outlineGenHint", {
                  from: pendingOutlineGen.from,
                  to: pendingOutlineGen.to,
                })
          }}
        </p>
        <div class="mt-3 min-h-0 flex-1 space-y-3 overflow-y-auto">
          <div
            v-if="pendingOutlineGen.loading"
            class="flex items-center gap-2 py-8 text-sm text-muted-foreground"
          >
            <Loader2 class="h-4 w-4 animate-spin" />
            {{ t("workspace.outlineGenLoading") }}
          </div>
          <template v-else>
            <div class="rounded-md border">
              <button
                type="button"
                class="flex w-full items-center justify-between gap-2 px-3 py-2 text-left text-xs font-medium text-muted-foreground hover:bg-muted/40"
                @click="pendingOutlineGen.materialsOpen = !pendingOutlineGen.materialsOpen"
              >
                <span>{{ t("workspace.outlineGenMaterialsLabel") }}</span>
                <span>{{ pendingOutlineGen.materialsOpen ? "▾" : "▸" }}</span>
              </button>
              <pre
                v-if="pendingOutlineGen.materialsOpen"
                class="max-h-[min(28vh,240px)] overflow-y-auto whitespace-pre-wrap break-words border-t px-3 py-2 text-xs text-muted-foreground"
              >{{ pendingOutlineGen.materials }}</pre>
            </div>
            <div>
              <label class="mb-1 block text-xs text-muted-foreground">{{
                t("workspace.outlineGenExpectationLabel")
              }}</label>
              <Textarea
                v-model="pendingOutlineGen.expectation"
                rows="6"
                class="min-h-[6rem] resize-y text-sm"
                :placeholder="t('workspace.outlineGenExpectationPh')"
                :aria-label="t('workspace.outlineGenExpectationLabel')"
              />
            </div>
          </template>
        </div>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" @click="cancelOutlineGen">
            {{ t("novels.cancel") }}
          </Button>
          <Button :disabled="pendingOutlineGen.loading" @click="confirmOutlineGen">
            {{ t("workspace.outlineGenConfirm") }}
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
            pendingCardDelete.step === 2
              ? t("workspace.deleteChapterConfirmAgain", { title: pendingCardDelete.title })
              : t("workspace.deleteCardConfirm", { title: pendingCardDelete.title })
          }}
        </p>
        <p
          v-if="pendingCardDelete.hasBody && pendingCardDelete.step === 1"
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
                : pendingCardDelete.hasBody && pendingCardDelete.step === 1
                  ? t("workspace.deleteContinue")
                  : t("workspace.deleteCard")
            }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 根 Chat：/清空章节 · /清空所有剧情（两步确认） -->
    <div
      v-if="pendingChatClear"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelPendingChatClear"
      @keydown.escape="cancelPendingChatClear"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">
          {{
            pendingChatClear.kind === "chapters"
              ? t("workspace.clearChaptersTitle")
              : t("workspace.clearPlotsTitle")
          }}
        </h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{
            pendingChatClear.kind === "chapters"
              ? pendingChatClear.step === 2
                ? t("workspace.clearChaptersConfirmAgain")
                : t("workspace.clearChaptersConfirm")
              : pendingChatClear.step === 2
                ? t("workspace.clearPlotsConfirmAgain")
                : t("workspace.clearPlotsConfirm")
          }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" @click="cancelPendingChatClear">
            {{ t("novels.cancel") }}
          </Button>
          <Button variant="destructive" @click="confirmPendingChatClear">
            {{
              pendingChatClear.step === 1
                ? t("workspace.deleteContinue")
                : t("workspace.clearConfirmAction")
            }}
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
                  :disabled="busy === 'regen-memory' || autoAll"
                  @click="selectAllMemoryRefs"
                >
                  {{ t("workspace.generateMemorySelectAll") }}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  :disabled="busy === 'regen-memory' || autoAll"
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
                    :disabled="busy === 'regen-memory' || autoAll"
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
                :disabled="busy === 'regen-memory' || autoAll"
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
              :disabled="busy === 'regen-memory' || autoAll"
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
              :disabled="!!busy || autoAll || !chapterMd.trim()"
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
