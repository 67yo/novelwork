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
import {
  BookOpen,
  Brain,
  Copy,
  GitBranch,
  LayoutGrid,
  Library,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Square,
  Trash2,
  User,
  X,
} from "@lucide/vue";
import { diffLines } from "@/lib/linediff";
import { renderChapterMd, stripChapterMeta } from "@/lib/md";
import { applyAutoLayout } from "@/lib/treeLayout";
import { formatChatContent } from "@/lib/chatFormat";

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
/** session-only per-card chat history */
const cardChats = ref<Record<string, { id: string; role: string; content: string }[]>>({});
const cardChatInput = ref("");
const prevN = ref(10);
/** 生成下一章大纲时一次写几章（1–12） */
const planNextCount = useLocalStorage("novework.planNextCount", 1);
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
  pendingGenerate.value = null;
  pendingRefine.value = null;
  if (n.kind === "chapter" || n.kind === "side_plot") {
    chapterMd.value = await api.getChapter(props.id, n.id);
  } else {
    chapterMd.value = "";
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
  repair_beats: "workspace.taskStepRepairBeats",
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
    if (!autoAll.value) clearChapterTaskUi();
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

/** 预生成确认：记忆章勾选 + 可编辑条件（改过即默认） */
const pendingGenerate = ref<{
  nodeId: string;
  loading: boolean;
  picks: { node_id: string; label: string; has_memory: boolean }[];
  selectedIds: string[];
} | null>(null);
const generateBrief = useLocalStorage("novework.generateBrief", "");

function factoryGenerateBrief(): string {
  return t("workspace.generateBriefDefault");
}

function ensureGenerateBrief() {
  if (!generateBrief.value.trim()) {
    generateBrief.value = factoryGenerateBrief();
  }
}

function resetGenerateBrief() {
  if (busy.value === "generate" || autoAll.value) return;
  generateBrief.value = factoryGenerateBrief();
}

function cancelGenerateConfirm() {
  if (busy.value === "generate") return;
  pendingGenerate.value = null;
}

function toggleGenerateMemoryId(id: string) {
  const p = pendingGenerate.value;
  if (!p || p.loading) return;
  const set = new Set(p.selectedIds);
  if (set.has(id)) set.delete(id);
  else set.add(id);
  pendingGenerate.value = { ...p, selectedIds: [...set] };
}

function selectAllGenerateMemory() {
  const p = pendingGenerate.value;
  if (!p || p.loading || !p.picks.length) return;
  pendingGenerate.value = { ...p, selectedIds: p.picks.map((c) => c.node_id) };
}

function deselectAllGenerateMemory() {
  const p = pendingGenerate.value;
  if (!p || p.loading) return;
  pendingGenerate.value = { ...p, selectedIds: [] };
}

async function openGenerateConfirm() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value || autoAll.value) {
    return;
  }
  if (pendingGenerate.value) return;
  cancelEditChapterBody();
  const nodeId = selected.value.id;
  ensureGenerateBrief();
  pendingGenerate.value = { nodeId, loading: true, picks: [], selectedIds: [] };
  try {
    const picks = await api.previewGenerateChapter(props.id, nodeId);
    if (!pendingGenerate.value || pendingGenerate.value.nodeId !== nodeId) return;
    // 默认勾选已有记忆的前序章
    const selectedIds = picks.filter((c) => c.has_memory).map((c) => c.node_id);
    pendingGenerate.value = { nodeId, loading: false, picks, selectedIds };
  } catch (e) {
    pendingGenerate.value = null;
    chapterResultNotice.value = String(e);
  }
}

async function executeGenerate(
  nodeId: string,
  memoryNodeIds: string[],
  userBrief: string,
): Promise<boolean> {
  cancelEditChapterBody();
  busy.value = "generate";
  beginChapterTask("context", 1, 3);
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

async function confirmGenerate() {
  const p = pendingGenerate.value;
  if (!p || p.loading) return;
  const { nodeId, selectedIds } = p;
  ensureGenerateBrief();
  const brief = generateBrief.value;
  pendingGenerate.value = null;
  await executeGenerate(nodeId, selectedIds, brief);
}

/** 自动全章：跳过确认框，用默认勾选（有记忆的前序章）+ 已存条件 */
async function generate(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  ensureGenerateBrief();
  let memoryIds: string[] = [];
  try {
    const picks = await api.previewGenerateChapter(props.id, selected.value.id);
    memoryIds = picks.filter((c) => c.has_memory).map((c) => c.node_id);
  } catch {
    /* 无前序或失败时仍继续预生成 */
  }
  return executeGenerate(selected.value.id, memoryIds, generateBrief.value);
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

const pendingRefine = ref<string | null>(null);
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

function resetRefineBrief() {
  if (busy.value === "refine" || autoAll.value) return;
  refineBrief.value = factoryRefineBrief();
}

function cancelRefineConfirm() {
  if (busy.value === "refine") return;
  pendingRefine.value = null;
}

function openRefineConfirm() {
  if (
    !selected.value ||
    selected.value.kind !== "chapter" ||
    !!busy.value ||
    autoAll.value ||
    !chapterMd.value
  ) {
    return;
  }
  if (pendingRefine.value) return;
  cancelEditChapterBody();
  ensureRefineBrief();
  pendingRefine.value = selected.value.id;
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

async function confirmRefine() {
  const nodeId = pendingRefine.value;
  if (!nodeId) return;
  ensureRefineBrief();
  const brief = refineBrief.value;
  pendingRefine.value = null;
  await executeRefine(nodeId, brief);
}

/** 自动全章：跳过确认框，用已存精修条件 */
async function refine(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  ensureRefineBrief();
  return executeRefine(selected.value.id, refineBrief.value);
}

/** 左侧「生成章节卡 / 生成下一章」确认：可编辑考虑材料 + 期望 */
const pendingOutlineGen = ref<{
  mode: "root" | "plan_next";
  count: number;
  nodeId: string | null;
  from: number;
  to: number;
  brief: string;
  loading: boolean;
  error: string;
} | null>(null);

async function openPlanNextBrief() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value || autoAll.value) return;
  if (pendingOutlineGen.value) return;
  const n = Math.min(12, Math.max(1, Number(planNextCount.value) || 1));
  planNextCount.value = n;
  const nodeId = selected.value.id;
  pendingOutlineGen.value = {
    mode: "plan_next",
    count: n,
    nodeId,
    from: 0,
    to: 0,
    brief: "",
    loading: true,
    error: "",
  };
  try {
    const r = await api.previewChapterOutlineBrief(props.id, "plan_next", n, nodeId);
    if (!pendingOutlineGen.value || pendingOutlineGen.value.mode !== "plan_next") return;
    pendingOutlineGen.value = {
      ...pendingOutlineGen.value,
      brief: r.brief,
      from: r.from,
      to: r.to,
      loading: false,
    };
  } catch (e) {
    pendingOutlineGen.value = null;
    chapterResultNotice.value = String(e);
  }
}

async function openRootGenBrief() {
  if (!selected.value || selected.value.kind !== "novel" || !!busy.value || autoAll.value) return;
  if (pendingOutlineGen.value) return;
  const maxN = Math.min(100, Math.max(1, novel.value?.chapter_count || 100));
  const n = Math.min(maxN, Math.max(1, Number(rootGenChapterCount.value) || 1));
  rootGenChapterCount.value = n;
  pendingOutlineGen.value = {
    mode: "root",
    count: n,
    nodeId: null,
    from: 1,
    to: n,
    brief: "",
    loading: true,
    error: "",
  };
  try {
    const r = await api.previewChapterOutlineBrief(props.id, "root", n);
    if (!pendingOutlineGen.value || pendingOutlineGen.value.mode !== "root") return;
    pendingOutlineGen.value = {
      ...pendingOutlineGen.value,
      brief: r.brief,
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

/** 左侧「生成剧情卡」两步确认 */
const pendingPlotGen = ref<{
  step: 1 | 2;
  nodeId: string;
  label: string;
  outline: string;
  userNotes: string;
} | null>(null);

function openPlotGenConfirm() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value || autoAll.value) return;
  if (pendingPlotGen.value || pendingOutlineGen.value) return;
  pendingPlotGen.value = {
    step: 1,
    nodeId: selected.value.id,
    label: selected.value.label,
    outline: selected.value.outline ?? "",
    userNotes: "",
  };
}

function cancelPlotGen() {
  pendingPlotGen.value = null;
}

function continuePlotGen() {
  const p = pendingPlotGen.value;
  if (!p || p.step !== 1) return;
  if (!p.outline.trim()) {
    chapterResultNotice.value = t("workspace.genPlotsNeedOutline");
    return;
  }
  pendingPlotGen.value = { ...p, step: 2 };
}

async function confirmPlotGen() {
  const p = pendingPlotGen.value;
  if (!p || p.step !== 2) return;
  const { nodeId, outline, userNotes } = p;
  pendingPlotGen.value = null;
  busy.value = "gen-plots";
  beginChapterTask("context", 1, 3);
  notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.generateChapterPlots(props.id, nodeId, outline, userNotes);
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const cur = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (cur) selected.value = cur;
    }
  } catch (e) {
    chapterResultNotice.value = isCancelledErr(e) ? t("workspace.chatStopped") : String(e);
  } finally {
    endChapterTask();
    busy.value = "";
  }
}

async function confirmOutlineGen() {
  const p = pendingOutlineGen.value;
  if (!p || p.loading) return;
  const brief = p.brief;
  const count = p.count;
  const mode = p.mode;
  const nodeId = p.nodeId;
  pendingOutlineGen.value = null;

  if (mode === "root") {
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
    return;
  }

  if (!nodeId) return;
  busy.value = "plan-next";
  beginChapterTask("context", 1, 3);
  notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.planNextChapters(props.id, nodeId, count, brief);
    chapterResultNotice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const cur = tree.value.nodes.find((x) => x.id === selected.value!.id);
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
  fetch_web: "workspace.chatStepFetchWeb",
  apply_character: "workspace.chatStepApplyCharacter",
  apply_outlines: "workspace.chatStepApplyOutlines",
  apply_card: "workspace.chatStepApplyCard",
  saving: "workspace.chatStepSaving",
};

function patchPending(pendingId: string, lines: string[]) {
  const i = messages.value.findIndex((m) => m.id === pendingId);
  if (i < 0) return;
  const next = [...messages.value];
  next[i] = { ...next[i], content: lines.join("\n") };
  messages.value = next;
}

async function sendChat() {
  const text = chatInput.value.trim();
  if (!text || busy.value === "chat") return;
  busy.value = "chat";
  notice.value = "";
  const now = new Date().toISOString();
  const pendingId = `pending-${Date.now()}`;
  const stepLines = [t("workspace.chatStepAnalyzing")];
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
      content: stepLines[0],
      created_at: now,
    },
  ];
  chatInput.value = "";
  await scrollChatBottom();

  const unlisten = await listen<{ novelId: string; step: string }>("chat-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    const key = CHAT_STEPS[ev.payload.step];
    if (!key) return;
    const line = `• ${t(key)}`;
    if (!stepLines.includes(line)) {
      stepLines.push(line);
      patchPending(pendingId, stepLines);
      void scrollChatBottom();
    }
  });

  try {
    messages.value = await api.chatSend(props.id, text);
    novel.value = await api.getNovel(props.id);
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    await scrollChatBottom();
  } catch (e) {
    const msg = String(e);
    if (msg.toLowerCase().includes("cancelled")) {
      patchPending(pendingId, [t("workspace.chatStopped")]);
      messages.value = await api.listChat(props.id);
    } else {
      patchPending(pendingId, [`${t("workspace.chatStepFailed")}\n${msg}`]);
      notice.value = msg;
    }
  } finally {
    unlisten();
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
  // 从 Vue Flow 内部状态取最新坐标（单向 :nodes 时 flowNodes 可能是旧的）
  for (const n of tree.value.nodes) {
    const gn = findNode(n.id);
    if (gn) n.position = { x: gn.position.x, y: gn.position.y };
  }
  pruneTreeRefs(tree.value);
  applyAutoLayout(tree.value.nodes, tree.value.edges);
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

async function sendCardChat() {
  const node = selected.value;
  if (!node || node.kind === "novel") return;
  const text = cardChatInput.value.trim();
  if (!text || busy.value === "card-chat") return;
  busy.value = "card-chat";
  notice.value = "";
  const pendingId = `pending-${Date.now()}`;
  const stepLines = [t("workspace.chatStepAnalyzing")];
  const prev = cardMessagesFor(node.id);
  setCardMessages(node.id, [
    ...prev,
    { id: `u-${Date.now()}`, role: "user", content: text },
    { id: pendingId, role: "assistant", content: stepLines[0] },
  ]);
  cardChatInput.value = "";
  await scrollChatBottom();

  const unlisten = await listen<{ novelId: string; step: string }>("chat-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    const key = CHAT_STEPS[ev.payload.step];
    if (!key) return;
    const line = `• ${t(key)}`;
    if (!stepLines.includes(line)) {
      stepLines.push(line);
      const list = cardMessagesFor(node.id).map((m) =>
        m.id === pendingId ? { ...m, content: stepLines.join("\n") } : m,
      );
      setCardMessages(node.id, list);
      void scrollChatBottom();
    }
  });

  try {
    const r = await api.cardChatSend(props.id, node.id, text);
    tree.value = r.tree;
    syncFlowFromTree();
    const updated = r.tree.nodes.find((n) => n.id === node.id);
    if (updated) {
      selected.value = updated;
      if (updated.kind === "chapter" || updated.kind === "side_plot") {
        chapterMd.value = await api.getChapter(props.id, updated.id);
      }
    }
    setCardMessages(node.id, [
      ...prev,
      { id: `u-${Date.now()}`, role: "user", content: text },
      { id: `a-${Date.now()}`, role: "assistant", content: r.reply },
    ]);
    await scrollChatBottom();
  } catch (e) {
    const msg = String(e);
    const content = msg.toLowerCase().includes("cancelled")
      ? t("workspace.chatStopped")
      : `${t("workspace.chatStepFailed")}\n${msg}`;
    const list = cardMessagesFor(node.id).map((m) =>
      m.id === pendingId ? { ...m, content } : m,
    );
    setCardMessages(node.id, list);
    if (!msg.toLowerCase().includes("cancelled")) notice.value = msg;
  } finally {
    unlisten();
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
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
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
    busy.value === "gen-cards",
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

const rootSlashCmds = computed<SlashCmd[]>(() => {
  const zh = locale.value.startsWith("zh") || locale.value === "ja";
  if (zh) {
    return [
      { cmd: "/章节卡", insert: "/章节卡 ", hint: t("workspace.slashHintChapters") },
      { cmd: "/添加剧情", insert: "/添加剧情 ", hint: t("workspace.slashHintAddPlot") },
      { cmd: "/剧情卡", insert: "/剧情卡 ", hint: t("workspace.slashHintPlots") },
      { cmd: "/清空章节", insert: "/清空章节", hint: t("workspace.slashHintClear") },
      { cmd: "/清空所有剧情", insert: "/清空所有剧情", hint: t("workspace.slashHintClearPlots") },
    ];
  }
  return [
    { cmd: "/chapters", insert: "/chapters ", hint: t("workspace.slashHintChapters") },
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
    ];
  }
  return [
    { cmd: "/enrich-plots", insert: "/enrich-plots", hint: t("workspace.slashHintEnrichPlots") },
  ];
});

const slashActive = ref(0);

/** 输入以 / 开头且尚未空格时，提示可补全指令 */
const slashSuggestions = computed(() => {
  const chapterCard = selected.value?.kind === "chapter";
  const cmds = chapterCard
    ? chapterSlashCmds.value
    : cardChatMode.value
      ? []
      : rootSlashCmds.value;
  if (!cmds.length) return [];
  const v = chapterCard ? cardChatInput.value : chatInput.value;
  if (!v.startsWith("/") || /\s/.test(v)) return [];
  const q = v.toLowerCase();
  return cmds.filter((c) => c.cmd.toLowerCase().startsWith(q) || c.cmd.startsWith(v));
});

watch(slashSuggestions, () => {
  slashActive.value = 0;
});

function applySlashCmd(cmd: SlashCmd) {
  if (selected.value?.kind === "chapter") cardChatInput.value = cmd.insert;
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

/** panel widths (px); middle gets the rest */
const leftW = ref(380);
const rightW = ref(320);
const MIN_SIDE = 220;
const MIN_MID = 280;

function startResize(which: "left" | "right", ev: MouseEvent) {
  ev.preventDefault();
  const startX = ev.clientX;
  const startLeft = leftW.value;
  const startRight = rightW.value;
  const onMove = (e: MouseEvent) => {
    const dx = e.clientX - startX;
    const total = window.innerWidth;
    if (which === "left") {
      leftW.value = Math.min(Math.max(startLeft + dx, MIN_SIDE), total - rightW.value - MIN_MID - 16);
    } else {
      rightW.value = Math.min(Math.max(startRight - dx, MIN_SIDE), total - leftW.value - MIN_MID - 16);
    }
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
    <div class="flex min-h-0 flex-1">
      <!-- 左：章节预览 -->
      <div class="flex min-h-0 flex-col border-r" :style="{ width: leftW + 'px', flex: '0 0 auto' }">
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
            <!-- 剧情卡 / 预生成 / 精修 -->
            <div class="grid grid-cols-3 gap-1.5">
              <Button
                size="sm"
                variant="outline"
                class="min-w-0 w-full px-1.5"
                :disabled="!!busy || autoAll || bodyEditing"
                :title="t('workspace.genPlotsHint')"
                @click="openPlotGenConfirm"
              >
                <Loader2 v-if="busy === 'gen-plots'" class="mr-1 h-3.5 w-3.5 shrink-0 animate-spin" />
                <span class="truncate">{{
                  busy === "gen-plots" ? t("workspace.genPlotsBusy") : t("workspace.genPlots")
                }}</span>
              </Button>
              <Button
                size="sm"
                class="min-w-0 w-full px-1.5"
                :disabled="!!busy || autoAll || bodyEditing"
                @click="openGenerateConfirm"
              >
                <Loader2 v-if="busy === 'generate'" class="mr-1 h-3.5 w-3.5 shrink-0 animate-spin" />
                <span class="truncate">{{
                  busy === "generate"
                    ? t("workspace.generating")
                    : chapterMd
                      ? t("workspace.regenerate")
                      : t("workspace.generate")
                }}</span>
              </Button>
              <Button
                size="sm"
                variant="secondary"
                class="min-w-0 w-full px-1.5"
                :disabled="!!busy || autoAll || !chapterMd || bodyEditing"
                @click="openRefineConfirm"
              >
                <Loader2 v-if="busy === 'refine'" class="mr-1 h-3.5 w-3.5 shrink-0 animate-spin" />
                <span class="truncate">{{
                  busy === "refine" ? t("workspace.refining") : t("workspace.refine")
                }}</span>
              </Button>
            </div>
            <div
              class="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md border bg-muted/40 px-2.5 py-1.5"
              :class="chapterBusy || autoAll ? 'pointer-events-none opacity-50' : ''"
              :title="t('workspace.refineModeHint')"
            >
              <span class="text-[11px] font-medium text-muted-foreground">{{ t("workspace.refineRef") }}</span>
              <div
                class="inline-flex rounded-md border bg-background p-0.5"
                role="group"
                :aria-label="t('workspace.refineModeHint')"
              >
                <button
                  type="button"
                  class="rounded px-2 py-1 text-[11px] leading-none transition-colors"
                  :class="
                    refineMode === 'memory'
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:text-foreground'
                  "
                  :aria-pressed="refineMode === 'memory'"
                  :disabled="chapterBusy || autoAll"
                  @click="refineMode = 'memory'"
                >
                  {{ t("workspace.refineModeMemory") }}
                </button>
                <button
                  type="button"
                  class="rounded px-2 py-1 text-[11px] leading-none transition-colors"
                  :class="
                    refineMode === 'full'
                      ? 'bg-primary text-primary-foreground'
                      : 'text-muted-foreground hover:text-foreground'
                  "
                  :aria-pressed="refineMode === 'full'"
                  :disabled="chapterBusy || autoAll"
                  @click="refineMode = 'full'"
                >
                  {{ t("workspace.refineModeFull") }}
                </button>
              </div>
              <div class="flex items-center gap-1 text-[11px] text-muted-foreground">
                <span>{{ t("workspace.refinePrev") }}</span>
                <Input
                  v-model.number="prevN"
                  type="number"
                  min="0"
                  max="20"
                  class="h-7 w-12 px-1.5 text-center text-xs"
                  :disabled="chapterBusy || autoAll"
                />
                <span>{{ t("workspace.chapters") }}</span>
              </div>
            </div>

            <!-- 大纲：生成下一章 -->
            <div class="flex flex-wrap items-center gap-1.5">
              <Button
                size="sm"
                variant="outline"
                class="min-w-0 flex-1"
                :disabled="!!busy || autoAll"
                :title="t('workspace.planNextHint')"
                @click="openPlanNextBrief"
              >
                <Loader2 v-if="busy === 'plan-next'" class="mr-1.5 h-3.5 w-3.5 shrink-0 animate-spin" />
                <span class="truncate">{{
                  busy === "plan-next" ? t("workspace.planningNext") : t("workspace.planNext")
                }}</span>
              </Button>
              <div
                class="flex shrink-0 items-center gap-1 rounded-md border bg-muted/40 px-2 py-1 text-[11px] text-muted-foreground"
                :class="chapterBusy || autoAll ? 'opacity-50' : ''"
                :title="t('workspace.planNextCountHint')"
              >
                <Input
                  v-model.number="planNextCount"
                  type="number"
                  min="1"
                  max="12"
                  class="h-7 w-11 px-1 text-center text-xs"
                  :disabled="!!busy || autoAll"
                  :aria-label="t('workspace.planNextCountHint')"
                />
                <span>{{ t("workspace.chapters") }}</span>
              </div>
            </div>

            <!-- 辅助：停止 / 复制 / 记忆 -->
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
              <p class="text-[11px] text-muted-foreground">{{ t("workspace.knowledgeFeaturesHint") }}</p>
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
                <Button
                  variant="outline"
                  size="sm"
                  :disabled="coverBusy"
                  @click="pickAndSetCover"
                >
                  <Loader2 v-if="coverBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  {{ t("workspace.pickCover") }}
                </Button>
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
        class="w-1.5 shrink-0 cursor-col-resize bg-border hover:bg-primary/40"
        :title="t('workspace.resize')"
        @mousedown="startResize('left', $event)"
      />

      <!-- 中：树图（缩放 / 拖动画布） -->
      <div
        class="relative min-h-0 min-w-0 flex-1"
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
        class="w-1.5 shrink-0 cursor-col-resize bg-border hover:bg-primary/40"
        :title="t('workspace.resize')"
        @mousedown="startResize('right', $event)"
      />

      <!-- 右：Chat（小说总聊 / 卡片专聊） -->
      <div class="flex min-h-0 flex-col" :style="{ width: rightW + 'px', flex: '0 0 auto' }">
        <div class="border-b px-3 py-2 text-sm font-medium">
          {{ cardChatMode ? cardChatTitle : t("workspace.chat") }}
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
          <div v-if="cardChatMode" class="relative">
            <ul
              v-if="slashSuggestions.length"
              class="absolute bottom-full left-0 right-0 z-20 mb-1 max-h-48 overflow-auto rounded-md border bg-popover py-1 text-sm shadow-md"
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
              class="absolute bottom-full left-0 right-0 z-20 mb-1 max-h-48 overflow-auto rounded-md border bg-popover py-1 text-sm shadow-md"
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
          <Button
            v-if="busy === 'chat' || busy === 'card-chat'"
            class="w-full"
            variant="destructive"
            @click="stopChat"
          >
            <Square class="mr-1.5 h-3.5 w-3.5" />
            {{ t("workspace.stop") }}
          </Button>
          <Button
            v-else
            class="w-full"
            @click="cardChatMode ? sendCardChat() : sendChat()"
          >
            {{ t("workspace.send") }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 生成剧情卡：两步确认（大纲 + 自定义 → 最终确认） -->
    <div
      v-if="pendingPlotGen"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelPlotGen"
    >
      <div
        class="flex w-full max-w-xl max-h-[min(90vh,720px)] flex-col rounded-lg border bg-background p-5 shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <h2 class="text-base font-semibold">{{ t("workspace.genPlotsTitle") }}</h2>
        <template v-if="pendingPlotGen.step === 1">
          <p class="mt-2 text-xs text-muted-foreground">
            {{ t("workspace.genPlotsStep1Hint", { title: pendingPlotGen.label }) }}
          </p>
          <label class="mt-3 block text-xs font-medium text-muted-foreground">
            {{ t("workspace.genPlotsOutlineLabel") }}
          </label>
          <Textarea
            v-model="pendingPlotGen.outline"
            rows="8"
            class="mt-1 min-h-[8rem] flex-1 resize-y text-sm"
            :aria-label="t('workspace.genPlotsOutlineLabel')"
          />
          <label class="mt-3 block text-xs font-medium text-muted-foreground">
            {{ t("workspace.genPlotsNotesLabel") }}
          </label>
          <Textarea
            v-model="pendingPlotGen.userNotes"
            rows="4"
            class="mt-1 min-h-[5rem] resize-y text-sm"
            :placeholder="t('workspace.genPlotsNotesPh')"
            :aria-label="t('workspace.genPlotsNotesLabel')"
          />
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="outline" @click="cancelPlotGen">{{ t("novels.cancel") }}</Button>
            <Button :disabled="!pendingPlotGen.outline.trim()" @click="continuePlotGen">
              {{ t("workspace.deleteContinue") }}
            </Button>
          </div>
        </template>
        <template v-else>
          <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
            {{ t("workspace.genPlotsStep2Body", { title: pendingPlotGen.label }) }}
          </p>
          <p class="mt-2 text-sm font-medium text-foreground">
            {{ t("workspace.genPlotsStep2Warn") }}
          </p>
          <div class="mt-5 flex justify-end gap-2">
            <Button variant="outline" @click="cancelPlotGen">{{ t("novels.cancel") }}</Button>
            <Button @click="confirmPlotGen">{{ t("workspace.genPlotsConfirm") }}</Button>
          </div>
        </template>
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
        <h2 class="text-base font-semibold">
          {{
            pendingOutlineGen.mode === "root"
              ? t("workspace.outlineGenTitleRoot")
              : t("workspace.outlineGenTitleNext")
          }}
        </h2>
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
        <div class="mt-3 min-h-0 flex-1">
          <div
            v-if="pendingOutlineGen.loading"
            class="flex items-center gap-2 py-8 text-sm text-muted-foreground"
          >
            <Loader2 class="h-4 w-4 animate-spin" />
            {{ t("workspace.outlineGenLoading") }}
          </div>
          <Textarea
            v-else
            v-model="pendingOutlineGen.brief"
            rows="18"
            class="h-[min(50vh,420px)] min-h-[12rem] resize-y text-sm"
            :aria-label="t('workspace.outlineGenBriefLabel')"
          />
        </div>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" @click="cancelOutlineGen">
            {{ t("novels.cancel") }}
          </Button>
          <Button
            :disabled="pendingOutlineGen.loading || !pendingOutlineGen.brief.trim()"
            @click="confirmOutlineGen"
          >
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

    <!-- 预生成确认：勾选记忆章 | 生成条件 -->
    <div
      v-if="pendingGenerate"
      class="fixed inset-0 z-50 flex items-stretch justify-center bg-black/30 p-0 sm:p-4 md:p-6"
      @click.self="cancelGenerateConfirm"
      @keydown.escape="cancelGenerateConfirm"
    >
      <div
        class="flex h-full w-full max-w-6xl flex-col border bg-background shadow-lg sm:h-[min(92vh,860px)] sm:rounded-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="t('workspace.generateConfirmTitle')"
      >
        <div class="flex shrink-0 items-center justify-between gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">{{ t("workspace.generateConfirmTitle") }}</h2>
            <p class="truncate text-xs text-muted-foreground">{{ selected?.label }}</p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            class="px-2"
            :aria-label="t('novels.cancel')"
            @click="cancelGenerateConfirm"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex min-h-0 flex-1 flex-col gap-3 p-4 md:flex-row md:gap-4">
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <div
              class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2"
            >
              <p class="min-w-0 text-xs font-medium text-muted-foreground">
                {{ t("workspace.generateMemoryPicksLabel") }}
              </p>
              <div
                v-if="!pendingGenerate.loading && pendingGenerate.picks.length"
                class="flex shrink-0 gap-1"
              >
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  @click="selectAllGenerateMemory"
                >
                  {{ t("workspace.generateMemorySelectAll") }}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs"
                  @click="deselectAllGenerateMemory"
                >
                  {{ t("workspace.generateMemoryDeselectAll") }}
                </Button>
              </div>
            </div>
            <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-3">
              <div
                v-if="pendingGenerate.loading"
                class="flex items-center gap-2 text-xs text-muted-foreground"
              >
                <Loader2 class="h-3.5 w-3.5 animate-spin" />
                {{ t("workspace.generateConfirmLoading") }}
              </div>
              <p
                v-else-if="!pendingGenerate.picks.length"
                class="text-xs text-muted-foreground"
              >
                {{ t("workspace.generateMemoryPicksEmpty") }}
              </p>
              <ul v-else class="space-y-2">
                <li
                  v-for="c in pendingGenerate.picks"
                  :key="c.node_id"
                  class="flex items-start gap-2 rounded-md border bg-background px-3 py-2 text-xs"
                >
                  <input
                    :id="'gen-mem-' + c.node_id"
                    type="checkbox"
                    class="mt-0.5 h-3.5 w-3.5 shrink-0"
                    :checked="pendingGenerate.selectedIds.includes(c.node_id)"
                    @change="toggleGenerateMemoryId(c.node_id)"
                  />
                  <label :for="'gen-mem-' + c.node_id" class="min-w-0 flex-1 cursor-pointer">
                    <span class="font-medium">{{ c.label }}</span>
                    <span
                      class="mt-0.5 block text-[11px]"
                      :class="c.has_memory ? 'text-muted-foreground' : 'text-amber-700 dark:text-amber-400'"
                    >
                      {{
                        c.has_memory
                          ? t("workspace.generateMemoryHas")
                          : t("workspace.generateMemoryMissing")
                      }}
                    </span>
                  </label>
                </li>
              </ul>
            </div>
          </div>
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <div
              class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2"
            >
              <label
                class="min-w-0 text-xs font-medium text-muted-foreground"
                for="generate-brief"
              >
                {{ t("workspace.generateBriefLabel") }}
              </label>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 shrink-0 px-2 text-xs"
                :title="t('workspace.generateBriefResetHint')"
                @click="resetGenerateBrief"
              >
                {{ t("workspace.generateBriefReset") }}
              </Button>
            </div>
            <Textarea
              id="generate-brief"
              v-model="generateBrief"
              class="min-h-0 flex-1 resize-none rounded-none border-0 bg-transparent text-xs leading-relaxed shadow-none focus-visible:ring-0"
              :placeholder="t('workspace.generateBriefPh')"
              :aria-label="t('workspace.generateBriefLabel')"
            />
          </div>
        </div>
        <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t px-4 py-3">
          <p class="min-w-0 flex-1 text-[11px] text-muted-foreground">
            {{ t("workspace.generateConfirmHint") }}
          </p>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Button size="sm" variant="outline" @click="cancelGenerateConfirm">
              {{ t("novels.cancel") }}
            </Button>
            <Button size="sm" :disabled="pendingGenerate.loading" @click="confirmGenerate">
              {{ t("workspace.generateConfirmStart") }}
            </Button>
          </div>
        </div>
      </div>
    </div>

    <!-- 精修确认：可编辑精修条件 -->
    <div
      v-if="pendingRefine"
      class="fixed inset-0 z-50 flex items-stretch justify-center bg-black/30 p-0 sm:p-4 md:p-6"
      @click.self="cancelRefineConfirm"
      @keydown.escape="cancelRefineConfirm"
    >
      <div
        class="flex h-full w-full max-w-2xl flex-col border bg-background shadow-lg sm:h-[min(80vh,640px)] sm:rounded-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="t('workspace.refineConfirmTitle')"
      >
        <div class="flex shrink-0 items-center justify-between gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">{{ t("workspace.refineConfirmTitle") }}</h2>
            <p class="truncate text-xs text-muted-foreground">{{ selected?.label }}</p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            class="px-2"
            :aria-label="t('novels.cancel')"
            @click="cancelRefineConfirm"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex min-h-0 flex-1 flex-col p-4">
          <div class="flex min-h-0 min-w-0 flex-1 flex-col rounded-md border bg-muted/10">
            <div
              class="flex shrink-0 items-center justify-between gap-2 border-b px-3 py-2"
            >
              <label
                class="min-w-0 text-xs font-medium text-muted-foreground"
                for="refine-brief"
              >
                {{ t("workspace.refineBriefLabel") }}
              </label>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 shrink-0 px-2 text-xs"
                :title="t('workspace.refineBriefResetHint')"
                @click="resetRefineBrief"
              >
                {{ t("workspace.refineBriefReset") }}
              </Button>
            </div>
            <Textarea
              id="refine-brief"
              v-model="refineBrief"
              class="min-h-[240px] flex-1 resize-none rounded-none border-0 bg-transparent text-xs leading-relaxed shadow-none focus-visible:ring-0"
              :placeholder="t('workspace.refineBriefPh')"
              :aria-label="t('workspace.refineBriefLabel')"
            />
          </div>
        </div>
        <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t px-4 py-3">
          <p class="min-w-0 flex-1 text-[11px] text-muted-foreground">
            {{ t("workspace.refineConfirmHint") }}
          </p>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Button size="sm" variant="outline" @click="cancelRefineConfirm">
              {{ t("novels.cancel") }}
            </Button>
            <Button size="sm" @click="confirmRefine">
              {{ t("workspace.refineConfirmStart") }}
            </Button>
          </div>
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
