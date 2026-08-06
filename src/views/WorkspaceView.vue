<script setup lang="ts">
import { computed, markRaw, nextTick, onMounted, ref, watch, type Ref } from "vue";
import {
  VueFlow,
  ConnectionMode,
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
import { Loader2 } from "@lucide/vue";

const props = defineProps<{ id: string }>();
const { t, locale } = useI18n();

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
const coverBusy = ref(false);

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
          }
        : n;
    return {
      id: n.id,
      type: "story",
      position: { ...n.position },
      data,
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

async function selectNode(n: TreeNode) {
  selected.value = n;
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

async function generate() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  busy.value = "generate";
  notice.value = "";
  try {
    const r = await api.generateChapter(props.id, selected.value.id);
    chapterMd.value = r.content;
    notice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
  } catch (e) {
    notice.value = String(e);
  } finally {
    busy.value = "";
  }
}

async function refine() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  busy.value = "refine";
  notice.value = "";
  try {
    const r = await api.refineChapter(props.id, selected.value.id, prevN.value);
    chapterMd.value = r.content;
    notice.value = r.message;
    tree.value = await api.getTree(props.id);
    syncFlowFromTree();
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
  } catch (e) {
    notice.value = String(e);
  } finally {
    busy.value = "";
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
    patchPending(pendingId, [`${t("workspace.chatStepFailed")}\n${String(e)}`]);
    notice.value = String(e);
  } finally {
    unlisten();
    busy.value = "";
  }
}

function emptyCharacter(): NonNullable<TreeNode["character"]> {
  return { role: "", personality: "", motto: "", gender: "", style: "", alignment: "" };
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
    const list = cardMessagesFor(node.id).map((m) =>
      m.id === pendingId ? { ...m, content: `${t("workspace.chatStepFailed")}\n${String(e)}` } : m,
    );
    setCardMessages(node.id, list);
    notice.value = String(e);
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

/** 本章直接链接的剧情 + 人物（含剧情卡上挂的人物）名称，供左侧 tag 展示 */
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
  const k = selected.value?.kind;
  if (k === "chapter") return t("workspace.cardChatChapter");
  if (k === "character") return t("workspace.cardChatCharacter");
  if (k === "side_plot") return t("workspace.cardChatPlot");
  return t("workspace.chat");
});
const cardChatPlaceholder = computed(() => {
  const k = selected.value?.kind;
  if (k === "chapter") return t("workspace.cardChatChapterPh");
  if (k === "character") return t("workspace.cardChatCharacterPh");
  if (k === "side_plot") return t("workspace.cardChatPlotPh");
  return t("workspace.chatPlaceholder");
});

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
            <Button size="sm" :disabled="!!busy" @click="generate">
              <Loader2 v-if="busy === 'generate'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              {{
                busy === "generate"
                  ? t("workspace.generating")
                  : chapterMd
                    ? t("workspace.regenerate")
                    : t("workspace.generate")
              }}
            </Button>
            <div class="flex items-center gap-1 text-xs" :class="chapterBusy ? 'opacity-50' : ''">
              <span>{{ t("workspace.refinePrev") }}</span>
              <Input
                v-model.number="prevN"
                type="number"
                min="0"
                max="20"
                class="h-8 w-14"
                :disabled="chapterBusy"
              />
              <span>{{ t("workspace.chapters") }}</span>
            </div>
            <Button size="sm" variant="secondary" :disabled="!!busy || !chapterMd" @click="refine">
              <Loader2 v-if="busy === 'refine'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
              {{ busy === "refine" ? t("workspace.refining") : t("workspace.refine") }}
            </Button>
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
          <pre
            v-else-if="chapterMd"
            class="whitespace-pre-wrap font-sans text-sm leading-relaxed"
            :class="chapterBusy ? 'opacity-40' : ''"
          >{{ chapterMd }}</pre>
          <div v-else class="text-sm text-muted-foreground">
            <template v-if="selectedIsChapter">{{ t("workspace.noBody") }}</template>
            <template v-else-if="selected?.kind === 'side_plot'">{{ t("workspace.sidePlotHint") }}</template>
            <template v-else-if="selected?.kind === 'novel'">{{ t("workspace.rootHint") }}</template>
            <template v-else>{{ t("workspace.clickNode") }}</template>
          </div>
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
          <Button size="sm" variant="secondary" class="h-7 text-xs" @click="addCard('chapter')">
            + {{ t("workspace.addChapter") }}
          </Button>
          <Button size="sm" variant="secondary" class="h-7 text-xs" @click="addCard('character')">
            + {{ t("workspace.addCharacter") }}
          </Button>
          <Button size="sm" variant="secondary" class="h-7 text-xs" @click="addCard('side_plot')">
            + {{ t("workspace.addPlot") }}
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
          <Textarea
            v-if="cardChatMode"
            v-model="cardChatInput"
            rows="3"
            :placeholder="cardChatPlaceholder"
          />
          <Textarea
            v-else
            v-model="chatInput"
            rows="3"
            :placeholder="t('workspace.chatPlaceholder')"
          />
          <Button
            class="w-full"
            :disabled="busy === 'chat' || busy === 'card-chat'"
            @click="cardChatMode ? sendCardChat() : sendChat()"
          >
            {{ t("workspace.send") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
