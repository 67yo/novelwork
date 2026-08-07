<script setup lang="ts">
import { computed, markRaw, nextTick, onMounted, ref, watch, type Ref } from "vue";
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
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type ChatMessage,
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
import { Copy, LayoutGrid, Loader2, Play, Square } from "@lucide/vue";
import { diffLines } from "@/lib/linediff";
import { renderChapterMd, stripChapterMeta } from "@/lib/md";
import { applyAutoLayout } from "@/lib/treeLayout";

const props = defineProps<{ id: string }>();
const { t, locale } = useI18n();
const { fitView } = useVueFlow({ id: "nove-workspace" });

const novel = ref<NovelProject | null>(null);
const tree = ref<NovelTree | null>(null);
const selected = ref<TreeNode | null>(null);
const chapterMd = ref("");
const messages = ref<ChatMessage[]>([]);
const chatInput = ref("");
const chatListEl = ref<HTMLElement | null>(null);
/** session-only per-card chat history */
const cardChats = ref<Record<string, { id: string; role: string; content: string }[]>>({});
const cardChatInput = ref("");
const prevN = ref(10);
const busy = ref("");
const notice = ref("");
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

function syncFlowFromTree() {
  const tr = tree.value;
  if (!tr) {
    flowNodes.value = [];
    flowEdges.value = [];
    return;
  }
  flowNodes.value = tr.nodes.map((n) => {
    const data =
      n.kind === "novel" && novel.value
        ? {
            ...n,
            word_count_min: n.word_count_min || novel.value.word_count_min,
            word_count_max: n.word_count_max || novel.value.word_count_max,
            chapter_count: n.chapter_count || novel.value.chapter_count,
          }
        : n;
    return {
      id: n.id,
      type: "story",
      position: { ...n.position },
      data,
      selected: n.id === selected.value?.id,
    };
  });
  flowEdges.value = tr.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    sourceHandle: e.source_handle ?? edgeDefaultSource(e.kind),
    targetHandle: e.target_handle ?? edgeDefaultTarget(e.kind),
    animated: e.kind === "side_plot",
    updatable: true,
    label: edgeDisplayLabel(e),
    style: edgeStyle(e.kind),
  }));
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
  return "";
}

function edgeDefaultSource(kind: string) {
  if (kind === "character") return "left";
  if (kind === "side_plot") return "right";
  return "bottom";
}

function edgeDefaultTarget(kind: string) {
  if (kind === "character") return "right";
  if (kind === "side_plot") return "left";
  return "top";
}

function edgeStyle(kind: string): Record<string, string> {
  if (kind === "character") return { stroke: "#d97706" };
  if (kind === "side_plot") return { stroke: "#0284c7" };
  return { stroke: "#3d6b4f" };
}

function kindFromNodes(sourceId: string, targetId: string): string {
  const a = nodeKind(sourceId);
  const b = nodeKind(targetId);
  if (!a || !b) return "chapter";
  if (a === "character" && b === "character") return "character";
  const hasChar = a === "character" || b === "character";
  const hasPlot = a === "side_plot" || b === "side_plot";
  const hasChapter = a === "chapter" || b === "chapter";
  const hasNovel = a === "novel" || b === "novel";
  if (hasChar && (hasPlot || hasChapter || hasNovel)) return "character";
  if (hasPlot && (hasChapter || hasNovel)) return "side_plot";
  return "chapter";
}

async function loadAll() {
  novel.value = await api.getNovel(props.id);
  tree.value = await api.getTree(props.id);
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
  if (!autoAll.value) notice.value = "";
  chapterResultNotice.value = "";
  copyHint.value = "";
  if (n.kind === "chapter" || n.kind === "side_plot") {
    chapterMd.value = await api.getChapter(props.id, n.id);
  } else {
    chapterMd.value = "";
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
}

function linkedNotice(kind: string) {
  notice.value = t("workspace.linkedNotice", {
    kind:
      kind === "character"
        ? t("workspace.role")
        : kind === "side_plot"
          ? t("workspace.plotCard")
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
  await api.saveTree(tree.value);
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
  await api.saveTree(tree.value);
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
  await api.saveTree(tree.value);
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
  await api.saveTree(tree.value);
  syncFlowFromTree();
  notice.value = t("workspace.edgeRemoved");
}

onMounted(loadAll);
watch(() => props.id, loadAll);
watch(locale, () => syncFlowFromTree());

async function generate(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  busy.value = "generate";
  if (!autoAll.value) notice.value = "";
  chapterResultNotice.value = "";
  try {
    const r = await api.generateChapter(props.id, selected.value.id);
    const id = selected.value.id;
    delete refineBeforeByNode.value[id];
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
    chapterResultNotice.value = String(e);
    return false;
  } finally {
    busy.value = "";
  }
}

async function refine(): Promise<boolean> {
  if (!selected.value || selected.value.kind !== "chapter") return false;
  busy.value = "refine";
  if (!autoAll.value) notice.value = "";
  chapterResultNotice.value = "";
  const id = selected.value.id;
  const before = chapterMd.value;
  try {
    const r = await api.refineChapter(props.id, id, prevN.value);
    refineBeforeByNode.value = { ...refineBeforeByNode.value, [id]: before };
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
    chapterResultNotice.value = String(e);
    return false;
  } finally {
    busy.value = "";
  }
}

function stopAutoAll() {
  if (!autoAll.value) return;
  autoAll.value = false;
  notice.value = t("workspace.autoAllStopped");
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

async function autoLayout() {
  if (!tree.value || autoAll.value) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  await api.saveTree(tree.value);
  syncFlowFromTree();
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
}

async function addCard(kind: "chapter" | "character" | "side_plot") {
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
      linked_character_ids: [],
      linked_side_plot_ids: [],
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
    const chars = tree.value.nodes.filter((n) => n.kind === "character").length;
    node = {
      id,
      kind,
      label: t("workspace.newCharacter"),
      outline: "",
      character: emptyCharacter(),
      linked_character_ids: [],
      linked_side_plot_ids: [],
      position: { x: 40, y: 80 + chars * 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    root.linked_character_ids.push(id);
    edge = {
      id: `e-${root.id}-${id}`,
      source: root.id,
      target: id,
      kind: "character",
      source_handle: "left",
      target_handle: "right",
    };
  } else {
    const plots = tree.value.nodes.filter((n) => n.kind === "side_plot").length;
    node = {
      id,
      kind,
      label: t("workspace.newPlot"),
      outline: "",
      character: null,
      linked_character_ids: [],
      linked_side_plot_ids: [],
      position: { x: root.position.x + 280, y: 80 + plots * 140 },
      word_count: 0,
      word_count_min: 0,
      word_count_max: 0,
      chapter_count: 0,
    };
    root.linked_side_plot_ids.push(id);
    edge = {
      id: `e-${root.id}-${id}`,
      source: root.id,
      target: id,
      kind: "side_plot",
      source_handle: "right",
      target_handle: "left",
    };
  }

  tree.value.nodes.push(node);
  tree.value.edges.push(edge);
  await api.saveTree(tree.value);
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
    }
  } catch (e) {
    notice.value = String(e);
  } finally {
    coverBusy.value = false;
  }
}

async function onDropCover(ev: DragEvent) {
  ev.preventDefault();
  notice.value = t("workspace.coverDropHint");
}

const selectedIsChapter = computed(() => selected.value?.kind === "chapter");
const selectedIsCharacter = computed(() => selected.value?.kind === "character");

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

const chapterBusy = computed(() => busy.value === "generate" || busy.value === "refine");
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
    ];
  }
  return [
    { cmd: "/chapters", insert: "/chapters ", hint: t("workspace.slashHintChapters") },
    { cmd: "/add-plot", insert: "/add-plot ", hint: t("workspace.slashHintAddPlot") },
    { cmd: "/plots", insert: "/plots ", hint: t("workspace.slashHintPlots") },
    { cmd: "/clear-chapters", insert: "/clear-chapters", hint: t("workspace.slashHintClear") },
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
  await api.saveTree(tree.value);
}
</script>

<template>
  <div class="flex h-full flex-col">
    <div class="flex items-center justify-between border-b px-4 py-2">
      <div class="min-w-0">
        <h1 class="truncate text-lg font-semibold">{{ novel?.title ?? "…" }}</h1>
        <p class="truncate text-xs text-muted-foreground">{{ novel?.synopsis }}</p>
      </div>
      <div class="flex items-center gap-2">
        <div
          class="flex h-14 w-10 items-center justify-center overflow-hidden rounded border bg-muted text-[10px] text-muted-foreground"
          @dragover.prevent
          @drop="onDropCover"
        >
          <img v-if="coverUrl" :src="coverUrl" class="h-full w-full object-cover" alt="" />
          <span v-else>{{ t("workspace.cover") }}</span>
        </div>
        <Button variant="outline" size="sm" :disabled="coverBusy" @click="pickAndSetCover">
          {{ t("workspace.pickCover") }}
        </Button>
      </div>
    </div>

    <div class="flex min-h-0 flex-1">
      <!-- 左：章节预览 -->
      <div class="flex min-h-0 flex-col border-r" :style="{ width: leftW + 'px', flex: '0 0 auto' }">
        <div class="shrink-0 space-y-2 border-b p-3">
          <div class="text-sm font-medium">{{ selected?.label ?? t("workspace.selectNode") }}</div>
          <p
            v-if="selected && !selectedIsCharacter && selected.outline"
            class="max-h-24 overflow-y-auto whitespace-pre-wrap text-xs text-muted-foreground"
          >
            {{ selected.outline }}
          </p>
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
          <div v-if="selectedIsChapter" class="flex flex-wrap items-center gap-2">
            <Button size="sm" :disabled="!!busy || autoAll" @click="generate">
              <Loader2 v-if="busy === 'generate'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              {{
                busy === "generate"
                  ? t("workspace.generating")
                  : chapterMd
                    ? t("workspace.regenerate")
                    : t("workspace.generate")
              }}
            </Button>
            <div class="flex items-center gap-1 text-xs" :class="chapterBusy || autoAll ? 'opacity-50' : ''">
              <span>{{ t("workspace.refinePrev") }}</span>
              <Input
                v-model.number="prevN"
                type="number"
                min="0"
                max="20"
                class="h-8 w-14"
                :disabled="chapterBusy || autoAll"
              />
              <span>{{ t("workspace.chapters") }}</span>
            </div>
            <Button size="sm" variant="secondary" :disabled="!!busy || autoAll || !chapterMd" @click="refine">
              <Loader2 v-if="busy === 'refine'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              {{ busy === "refine" ? t("workspace.refining") : t("workspace.refine") }}
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="px-2"
              :disabled="!!busy || autoAll || !chapterMd"
              :title="t('workspace.copyBody')"
              :aria-label="t('workspace.copyBody')"
              @click="copyChapterBody"
            >
              <Copy class="h-3.5 w-3.5" />
            </Button>
            <span v-if="copyHint" class="text-[11px] text-muted-foreground">{{ copyHint }}</span>
          </div>
          <p v-if="chapterBusy" class="flex items-center gap-1.5 text-xs text-primary">
            <Loader2 class="h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.waitingResult") }}
          </p>
          <p v-else-if="notice" class="text-xs text-muted-foreground">{{ notice }}</p>
        </div>
        <div class="relative min-h-0 flex-1 overflow-auto p-4">
          <div
            v-if="chapterBusy"
            class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-background/70 backdrop-blur-[1px]"
          >
            <Loader2 class="h-6 w-6 animate-spin text-primary" />
            <p class="text-sm text-muted-foreground">
              {{ busy === "refine" ? t("workspace.refining") : t("workspace.generating") }}
            </p>
            <p class="text-xs text-muted-foreground">{{ t("workspace.waitingResult") }}</p>
          </div>
          <CharacterCardPanel
            v-if="selectedIsCharacter"
            :name="selected?.label ?? ''"
            :card="selected?.character ?? emptyCharacter()"
          />
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
          <div v-else class="text-sm text-muted-foreground">
            <template v-if="selectedIsChapter">{{ t("workspace.noBody") }}</template>
            <template v-else-if="selected?.kind === 'side_plot'">{{ t("workspace.sidePlotHint") }}</template>
            <template v-else-if="selected?.kind === 'novel'">{{ t("workspace.rootHint") }}</template>
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
      <div class="relative min-h-0 min-w-0 flex-1">
        <div class="absolute left-2 top-2 z-10 flex flex-wrap items-center gap-1">
          <Button size="sm" variant="secondary" class="h-7 text-xs" :disabled="autoAll" @click="addCard('chapter')">
            + {{ t("workspace.addChapter") }}
          </Button>
          <Button size="sm" variant="secondary" class="h-7 text-xs" :disabled="autoAll" @click="addCard('character')">
            + {{ t("workspace.addCharacter") }}
          </Button>
          <Button size="sm" variant="secondary" class="h-7 text-xs" :disabled="autoAll" @click="addCard('side_plot')">
            + {{ t("workspace.addPlot") }}
          </Button>
          <Button
            size="sm"
            variant="outline"
            class="h-7 text-xs"
            :disabled="autoAll || !tree"
            :title="t('workspace.autoLayout')"
            @click="autoLayout"
          >
            <LayoutGrid class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.autoLayout") }}
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
          v-model:nodes="flowNodes"
          v-model:edges="flowEdges"
          :node-types="nodeTypes"
          :connection-mode="ConnectionMode.Loose"
          :edges-updatable="true"
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
                class="whitespace-pre-wrap"
                :class="m.id.startsWith('pending-') ? 'italic text-muted-foreground' : ''"
              >
                {{ m.content }}
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
                class="whitespace-pre-wrap"
                :class="m.id.startsWith('pending-') ? 'italic text-muted-foreground' : ''"
              >
                {{ m.content }}
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
  </div>
</template>
