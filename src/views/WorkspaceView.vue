<script setup lang="ts">
import {
  computed,
  nextTick,
  onMounted,
  onUnmounted,
  provide,
  ref,
  watch,
} from "vue";
import { useLocalStorage, useDebounceFn } from "@vueuse/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type ChapterMemoryGroup,
  type PublicKnowledgeCard,
  type NovelProject,
  type NovelTree,
  type ChapterShot,
  type TreeEdge,
  type TreeNode,
} from "@/lib/api";
import type { MessageKey } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Separator } from "@/components/ui/separator";
import WorkspaceTabBar, { type WorkspaceTabId } from "@/components/workspace/WorkspaceTabBar.vue";
import ReteCanvas from "@/components/flow/ReteCanvas.vue";
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
import HistoryCulturePanel from "@/components/HistoryCulturePanel.vue";
import WorldReligionPanel from "@/components/WorldReligionPanel.vue";
import MajorEventPanel from "@/components/MajorEventPanel.vue";
import WorldviewChatPanel from "@/components/WorldviewChatPanel.vue";
import StoryRulesHubPanel from "@/components/StoryRulesHubPanel.vue";
import VolumePanel from "@/components/VolumePanel.vue";
import StoryRulesBlockPanel from "@/components/StoryRulesBlockPanel.vue";
import StoryRulesChatPanel from "@/components/StoryRulesChatPanel.vue";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";
import NovelFeaturesPicker from "@/components/NovelFeaturesPicker.vue";
import ChapterBodyEditor from "@/components/ChapterBodyEditor.vue";
import {
  emptyNovelFeatures,
  normalizeNovelFeatures,
  type NovelFeatures,
} from "@/lib/novelFeatures";
import {
  emptyCharacterCard,
  normalizeCharacterCard,
  syncLegacyFields,
} from "@/lib/characterCard";
import { useI18n } from "@/i18n";
import { paperSchemeStyle, usePaperScheme } from "@/lib/paperScheme";
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
import type { HistoryCultureData, MajorEvent, WorldReligion } from "@/lib/historyCulture";
import {
  isMajorEventSlot,
  isReligionSlot,
  normalizeMajorEvent,
  normalizeReligion,
} from "@/lib/historyCulture";
import {
  createMajorEventCard,
  createReligionCard,
  historyCultureHostId,
  isHistoryCultureChildSlot,
  linkedMajorEventCards,
  linkedReligionCards,
  majorEventsFromLinkedCards,
  migrateInlineHistoryCultureToCards,
  religionsFromLinkedCards,
  syncHistoryCultureExtracted,
} from "@/lib/historyCultureCards";
import {
  isFactionSlot,
  isRaceSlot,
  normalizeFaction,
  normalizeRace,
} from "@/lib/socialPower";
import {
  BookOpen,
  Brain,
  Clapperboard,
  ChevronDown,
  ChevronUp,
  Copy,
  GitBranch,
  Hash,
  Layers,
  LayoutGrid,
  Library,
  Lightbulb,
  ListTree,
  Loader2,
  PanelBottom,
  PanelRight,
  Pause,
  Play,
  Plus,
  Search,
  RefreshCw,
  Share2,
  Sparkles,
  Square,
  Trash2,
  User,
  X,
} from "@lucide/vue";
import { renderChapterMd, stripChapterMeta } from "@/lib/md";
import { chapterSpan, countWords } from "@/lib/chapterWordStats";
import { applyDiffPick, diffLines, groupDiffHunks } from "@/lib/linediff";
import {
  bodySuggestContext,
  insertBodySuggestion,
  replaceBodyLine,
  splitBodyLines,
} from "@/lib/chapterParagraphs";
import {
  chapterInheritedCharacterIds,
  chapterInheritedPlotIds,
  chapterLocalCharacterIds,
  chapterLocalPlotIds,
  chapterParentVolumeId,
  accordionCollapsedVolumeIds,
  applyAutoLayout,
  collapsedVolumeHiddenIds,
  rootLinkedCharacterIds,
  rootLinkedPlotIds,
  volumeChapterIds,
  volumeLocalCharacterIds,
  volumeLocalPlotIds,
} from "@/lib/treeLayout";
import {
  WORLDVIEW_FAN_SLOTS,
  ensureWorldviewCards,
  isFixedRootKnowledgeEdge,
  isFixedRootKnowledgeSlot,
  isStoryRulesSlot,
  isWorldviewFanSlot,
  knowledgeSlot,
  missingWorldviewSlots,
  worldviewComplete,
  worldviewFanTitle,
} from "@/lib/worldview";
import {
  ensureWritePromptsCard,
  isWritePromptsRootEdge,
  isWritePromptsSlot,
  writePromptCardsForSide,
  writePromptEdgeHandles,
  writePromptsHub,
  writePromptsLinkHost,
  type WritePromptKind,
} from "@/lib/writePrompts";
import { migrateRootSpecialEdges } from "@/lib/flowSockets";
import type { FlowCanvasEdge, FlowCanvasNode, FlowConnect } from "@/lib/flowCanvas";
import {
  formatStoryRulesBlockExtracted,
  isStoryRulesFanSlot,
  normalizeStoryRulesBlock,
  type ConstraintRedlinesData,
  type FulfillmentSystemData,
  type StoryEngineData,
  type StoryRulesBlockSlot,
  type SurfaceSettingData,
} from "@/lib/storyRules";
import { emptyVolume, normalizeVolume, type VolumeData } from "@/lib/volume";
import {
  ensureStoryRulesFanCards,
  findStoryRulesNode,
  linkedStoryRulesBlocks,
  syncStoryRulesParentExtracted,
} from "@/lib/storyRulesCards";
import { storyRulesBlockSlotOf } from "@/lib/storyRulesGen";
import {
  chatCmdGenerateChapter,
  chatCmdGenerateOutline,
  chatCmdRefineChapter,
} from "@/lib/chapterChatCommands";
import { enqueueGlobalChatSend } from "@/lib/globalChatBridge";
import {
  buildBookNavItems,
  buildCharacterNavItems,
  buildKnowledgeNavItems,
  buildPlotNavItems,
  buildStoryRulesNavItems,
  buildWorldviewNavItems,
  knowledgeNodeLabel,
} from "@/lib/knowledgeListSort";

const props = defineProps<{ id: string }>();
const { t, locale } = useI18n();
const { scheme: paperScheme } = usePaperScheme();
const workspaceTab = useLocalStorage<WorkspaceTabId>("novework.workspaceTab", "book");
const lastWorkspaceNode = useLocalStorage<Record<string, string>>("novework.lastWorkspaceNode", {});
const reteCanvas = ref<{
  fitView: (opts?: { nodes?: string[] }) => void | Promise<void>;
} | null>(null);

function fitView(opts?: { padding?: number; duration?: number; nodes?: string[]; maxZoom?: number }) {
  void reteCanvas.value?.fitView({ nodes: opts?.nodes });
}

/** 画布浮动导航显隐（章节 / 分卷 / 人物 / 剧情 / 知识） */
const showChapterNav = useLocalStorage("novework.showChapterNav", true);
type CanvasNavTab = "chapter" | "volume" | "character" | "plot" | "knowledge";
const canvasNavTab = useLocalStorage<CanvasNavTab>("novework.canvasNavTab", "chapter");

const novel = ref<NovelProject | null>(null);
const tree = ref<NovelTree | null>(null);
const selected = ref<TreeNode | null>(null);
const publicKnowledgeCards = ref<PublicKnowledgeCard[]>([]);
const publicPickOpen = ref(false);
const publicPickFilter = ref("");
const publicPickBusy = ref(false);
/** 生成/精修编辑里 + 公共卡时指定左右侧；全书/结构树添加则为 null（挂根）。 */
const publicPickWriteSide = ref<WritePromptKind | null>(null);
const publishBusy = ref(false);
const chapterMd = ref("");
/** 章节正文手动编辑 */
const bodyEditing = ref(false);
const bodyDraft = ref("");
const bodySaveBusy = ref(false);
const bodyAutosaved = ref(false);
/** Chat 写章后：编辑 / 更新对比 */
const bodyView = ref<"edit" | "diff">("edit");
const bodyDiffBefore = ref("");
const bodyDiffAfter = ref("");
/** 点击生成/精修时的正文，避免多次 set 把「更新前」冲成新稿 */
const bodyDiffOrigin = ref<{ nodeId: string; before: string } | null>(null);
let contentChangePending: { nodeId: string; before: string } | null = null;
let contentChangeFlush: ReturnType<typeof setTimeout> | null = null;
const bodyDiffHunks = computed(() => {
  if (!bodyDiffBefore.value && !bodyDiffAfter.value) return [];
  return diffLines(bodyDiffBefore.value, bodyDiffAfter.value);
});
const bodyDiffBlocks = computed(() => groupDiffHunks(bodyDiffHunks.value));
const bodyDiffReady = computed(() => bodyDiffHunks.value.some((h) => h.type !== "eq"));
let chapterContentGen = 0;
/** idle | loading（下模型/推理）| playing */
const bodyTts = ref<"idle" | "loading" | "playing">("idle");
/** v1.1 语速为整数 1–3 */
const ttsSpeed = useLocalStorage("novework.ttsSpeed", 1);
type TtsDownloadProgress = {
  phase?: string;
  file?: string;
  fileIndex?: number;
  fileTotal?: number;
  bytesDownloaded?: number;
  bytesTotal?: number | null;
  percent?: number;
};
const bodyTtsDownload = ref<TtsDownloadProgress | null>(null);
const bodyEditor = ref<InstanceType<typeof ChapterBodyEditor> | null>(null);
const pendingParaRewrite = ref<{ index: number; text: string } | null>(null);
const paraRewriteNote = ref("");
const paraRewriteBusy = ref(false);
const paraRewriteError = ref("");
const bodySuggestOn = useLocalStorage("novework.bodySuggestOn", false);
const bodySuggestBusy = ref(false);
const bodySuggestError = ref("");
const bodySuggestItems = ref<string[]>([]);
let bodySuggestInsertAt = 0;
let bodySuggestGen = 0;
/** 上次成功落盘的正文快照；用于跳过无变更的自动保存 */
let lastSavedBody = "";
/** 按小说记住折叠的分卷（只藏画布，不改树） */
const collapsedVolumesByNovel = useLocalStorage<Record<string, string[]>>(
  "novework.collapsedVolumesByNovel",
  {},
);
const collapsedVolumeIdSet = computed(() => new Set(collapsedVolumesByNovel.value[props.id] ?? []));
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
const wordStatsOpen = ref(false);
const wordStatsMode = ref<"all" | "range">("all");
const wordStatsFrom = ref(1);
const wordStatsTo = ref(1);
const wordStatsBusy = ref(false);
const wordStatsError = ref("");
const wordStatsResult = ref<{
  from: number;
  to: number;
  total: number;
  items: { label: string; words: number }[];
} | null>(null);
/** null itemIndex = 清除整章记忆 */
const pendingMemoryDelete = ref<{
  nodeId: string;
  label: string;
  itemIndex: number | null;
} | null>(null);
const deletingMemory = ref(false);
const shotsPanelOpen = ref(false);
const shots = ref<ChapterShot[]>([]);
const shotsBusy = ref(false);
const shotsError = ref("");
const shotsNotice = ref("");
const shotsTotalSec = computed(() =>
  shots.value.reduce((sum, s) => sum + (Number(s.duration_sec) || 0), 0),
);
const shotsPreviewTitle = computed(() =>
  shotsTotalSec.value > 0
    ? t("workspace.shotsPreviewWithTotal", { n: shotsTotalSec.value })
    : t("workspace.shotsPreview"),
);
const shotsPanelHeading = computed(() =>
  shotsTotalSec.value > 0
    ? t("workspace.shotsPanelTitleWithTotal", { n: shotsTotalSec.value })
    : t("workspace.shotsPanelTitle"),
);
const pendingShotSplit = ref<1 | 2 | null>(null);
const busy = ref("");
const notice = ref("");
/** 章节 AI 任务等待：当前阶段与进度 */
const chapterProgress = ref<{
  step: string;
  index: number;
  total: number;
  detail?: string;
} | null>(null);
/** 当前 AI 请求已发送（prompt）token；confirmed=API 实值，否则为估算 */
const chapterTokens = ref<{
  prompt: number;
  completion: number;
  confirmed: boolean;
} | null>(null);
/** 每步耗时（最后一步 done=false 时为进行中） */
const chapterStepTimings = ref<
  { step: string; ms: number; done: boolean; detail?: string }[]
>([]);
let chapterStepStartedAt = 0;
let chapterTickTimer: ReturnType<typeof setInterval> | null = null;
/** 驱动进行中步骤的秒表刷新 */
const chapterTick = ref(0);
let unlistenChapterProgress: (() => void) | null = null;
let unlistenChapterTokens: (() => void) | null = null;
let unlistenTreeChanged: (() => void) | null = null;
let unlistenChapterContent: (() => void) | null = null;
let unlistenChapterTts: (() => void) | null = null;
let unlistenChapterTtsDownload: (() => void) | null = null;
let unlistenChapterTtsChunk: (() => void) | null = null;
/** AI 任务结束后的结束语（左侧底部独立区） */
const chapterResultNotice = ref("");
const coverBusy = ref(false);
const coverPromptBusy = ref(false);
const coverPrompt = ref("");
const coverPromptHint = ref("");
const copyHint = ref("");
const flowNodes = ref<FlowCanvasNode[]>([]);
const flowEdges = ref<FlowCanvasEdge[]>([]);

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
    volume: n.volume ? normalizeVolume(n.volume) : n.volume,
    detailed_outline: [...(n.detailed_outline ?? [])],
    linked_character_ids: [...(n.linked_character_ids ?? [])],
    linked_side_plot_ids: [...(n.linked_side_plot_ids ?? [])],
    linked_knowledge_ids: [...(n.linked_knowledge_ids ?? [])],
    position: { ...n.position },
  };
}

/** 丢掉指向已删节点的边与 linked_*；章节/分卷剥离继承剧情与人物（只读）。知识卡不继承。 */
function pruneTreeRefs(tr: NovelTree) {
  const alive = new Set(tr.nodes.map((n) => n.id));
  tr.edges = tr.edges.filter((e) => alive.has(e.source) && alive.has(e.target));
  for (const n of tr.nodes) {
    n.linked_character_ids = (n.linked_character_ids ?? []).filter((id) => alive.has(id));
    n.linked_side_plot_ids = (n.linked_side_plot_ids ?? []).filter((id) => alive.has(id));
    n.linked_knowledge_ids = (n.linked_knowledge_ids ?? []).filter((id) => alive.has(id));
    if (n.kind === "chapter") {
      const inheritedPlots = new Set(chapterInheritedPlotIds(n.id, tr.nodes, tr.edges));
      n.linked_side_plot_ids = n.linked_side_plot_ids.filter((id) => !inheritedPlots.has(id));
      const inheritedChars = new Set(chapterInheritedCharacterIds(n.id, tr.nodes, tr.edges));
      n.linked_character_ids = n.linked_character_ids.filter((id) => !inheritedChars.has(id));
    } else if (n.kind === "volume") {
      const rootPlots = new Set(rootLinkedPlotIds(tr.nodes, tr.edges));
      n.linked_side_plot_ids = n.linked_side_plot_ids.filter((id) => !rootPlots.has(id));
      const rootChars = new Set(rootLinkedCharacterIds(tr.nodes, tr.edges));
      n.linked_character_ids = n.linked_character_ids.filter((id) => !rootChars.has(id));
    }
  }
}

function toggleVolumeCollapse(volumeId: string) {
  const alive = (tree.value?.nodes ?? []).filter((n) => n.kind === "volume").map((n) => n.id);
  const cur = collapsedVolumesByNovel.value[props.id] ?? [];
  const expand = cur.includes(volumeId) ? volumeId : null;
  collapsedVolumesByNovel.value = {
    ...collapsedVolumesByNovel.value,
    [props.id]: accordionCollapsedVolumeIds(alive, cur, expand),
  };
  syncFlowFromTree();
  nextTick(() => {
    void fitView({ padding: 0.18, duration: 280 });
  });
}

function syncFlowFromTree() {
  const tr = tree.value;
  if (!tr) {
    flowNodes.value = [];
    flowEdges.value = [];
    return;
  }
  const aliveVols = tr.nodes.filter((n) => n.kind === "volume").map((n) => n.id);
  const stored = collapsedVolumesByNovel.value[props.id] ?? [];
  const pruned = accordionCollapsedVolumeIds(aliveVols, stored);
  const same =
    pruned.length === stored.length && pruned.every((id) => stored.includes(id));
  if (!same) {
    collapsedVolumesByNovel.value = { ...collapsedVolumesByNovel.value, [props.id]: pruned };
  }
  const collapsed = new Set(pruned);
  const hiddenIds = collapsedVolumeHiddenIds(collapsed, tr.nodes, tr.edges);
  if (selected.value && hiddenIds.has(selected.value.id)) {
    const vid =
      selected.value.kind === "chapter"
        ? chapterParentVolumeId(selected.value.id, tr.nodes, tr.edges)
        : null;
    const vol = vid ? tr.nodes.find((n) => n.id === vid) : null;
    selected.value = vol ?? tr.nodes.find((n) => n.kind === "novel") ?? null;
    leaveChapterBodyEdit();
    if (selected.value) void api.setWorkspaceSelection(props.id, selected.value.id);
    else void api.setWorkspaceSelection(props.id, null);
  }
  const visNodes = tr.nodes.filter((n) => !hiddenIds.has(n.id)).map(cloneTreeNodeData);
  const visEdges = tr.edges.filter((e) => !hiddenIds.has(e.source) && !hiddenIds.has(e.target));
  const nodes = visNodes.map((n) => {
    const extra =
      n.kind === "volume"
        ? {
            volumeCollapsed: collapsed.has(n.id),
            collapsedChapterCount: volumeChapterIds(n.id, tr.nodes, tr.edges).length,
          }
        : {};
    const data =
      n.kind === "novel" && novel.value
        ? {
            ...n,
            word_count_min: n.word_count_min || novel.value.word_count_min,
            word_count_max: n.word_count_max || novel.value.word_count_max,
            chapter_count: n.chapter_count || novel.value.chapter_count,
            cover_url: novel.value.cover_path
              ? convertFileSrc(novel.value.cover_path)
              : "",
            toggleVolumeCollapse,
            ...extra,
          }
        : { ...n, toggleVolumeCollapse, ...extra };
    return {
      id: n.id,
      position: { ...n.position },
      data,
      selected: n.id === selected.value?.id,
    };
  });
  const edges = visEdges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    sourceHandle: e.source_handle ?? edgeDefaultSource(e.kind),
    targetHandle: e.target_handle ?? edgeDefaultTarget(e.kind),
    kind: e.kind,
    locked: isFixedRootKnowledgeEdge(tr.nodes, e),
    label: edgeDisplayLabel(e),
    style: edgeStyle(e.kind),
  }));
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
  if (a === "knowledge" && b === "knowledge") return "knowledge";
  if (hasChar && (hasPlot || host)) return "character";
  if (hasPlot && host) return "side_plot";
  if (a === "volume" && b === "volume") return "volume";
  if (hasVolume && (hasNovel || hasChapter)) return hasNovel && hasVolume ? "volume" : "chapter";
  return "chapter";
}

function rememberWorkspaceNode(nodeId: string) {
  lastWorkspaceNode.value = { ...lastWorkspaceNode.value, [props.id]: nodeId };
}

async function loadAll() {
  novel.value = await api.getNovel(props.id);
  let tr = await api.getTree(props.id);
  const savedId = lastWorkspaceNode.value[props.id];
  const savedEarly = savedId ? tr.nodes.find((n) => n.id === savedId) : undefined;
  // Pin MCP "current novel" before slow migrate — Chat/get_novel_info must not keep the previous book.
  const rootEarly = tr.nodes.find((n) => n.kind === "novel");
  const chapterEarly = tr.nodes.find((n) => n.kind === "chapter");
  const pin =
    savedEarly ??
    (workspaceTab.value === "manuscript" ? (chapterEarly ?? rootEarly) : (rootEarly ?? chapterEarly));
  if (pin) await api.setWorkspaceSelection(props.id, pin.id);
  tr = await maybeMigrateWorldview(tr);
  tree.value = tr;
  publicKnowledgeCards.value = await api.listPublicKnowledgeCards().catch(() => []);
  syncFlowFromTree();
  const saved = savedId ? tr.nodes.find((n) => n.id === savedId) : undefined;
  if (saved && selectionFitsTab(saved, workspaceTab.value)) await selectNode(saved);
  else await applyTabDefaultSelection(true);
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
  const titleFn = (key: string) => t(key as Parameters<typeof t>[0]);
  const wv = ensureWorldviewCards(tr, titleFn);
  const wp = ensureWritePromptsCard(tr, titleFn);
  const sr = ensureStoryRulesFanCards(tr, titleFn);
  const ax = migrateInlineAxiomsToCards(tr);
  const loc = migrateInlineLocationsToCards(tr);
  const sp = migrateInlineSocialPowerToCards(tr);
  const hc = migrateInlineHistoryCultureToCards(tr);
  const handles = migrateRootSpecialEdges(tr);
  if (!ax && !loc && !sp && !hc && !wv && !sr && !wp && !handles) return tr;
  return persistTree(tr);
}

async function selectNode(n: TreeNode) {
  if (tree.value && n.kind === "chapter") {
    const vid = chapterParentVolumeId(n.id, tree.value.nodes, tree.value.edges);
    if (vid && collapsedVolumeIdSet.value.has(vid)) toggleVolumeCollapse(vid);
  }
  const prevId = selected.value?.id;
  const resolved = tree.value?.nodes.find((x) => x.id === n.id) ?? n;
  selected.value = resolved;
  rememberWorkspaceNode(n.id);
  await api.setWorkspaceSelection(props.id, n.id);
  notice.value = "";
  chapterResultNotice.value = "";
  copyHint.value = "";
  memoryPanelOpen.value = false;
  pendingParaRewrite.value = null;
  if (prevId !== n.id) clearBodyDiff();
  if (n.kind === "chapter" || n.kind === "side_plot") {
    chapterMd.value = await api.getChapter(props.id, n.id);
  } else {
    chapterMd.value = "";
  }
  if (n.kind === "chapter") {
    enterChapterBodyEdit();
    void loadChapterShots(n.id, false);
  } else {
    leaveChapterBodyEdit();
    shots.value = [];
  }
}

function isWorldviewRelatedNode(n: TreeNode): boolean {
  if (n.kind !== "knowledge") return false;
  const s = knowledgeSlot(n);
  return (
    isWorldviewFanSlot(s) ||
    isAxiomSlot(s) ||
    isLocationSlot(s) ||
    isRaceSlot(s) ||
    isFactionSlot(s) ||
    isReligionSlot(s) ||
    isMajorEventSlot(s)
  );
}

function isBookKnowledgeNode(n: TreeNode): boolean {
  if (n.kind !== "knowledge") return false;
  if (isWorldviewRelatedNode(n)) return false;
  const s = knowledgeSlot(n);
  if (isStoryRulesSlot(s) || isStoryRulesFanSlot(s)) return false;
  return true;
}

function selectionFitsTab(n: TreeNode, tab: WorkspaceTabId): boolean {
  if (tab === "book") return n.kind === "novel" || isBookKnowledgeNode(n);
  if (tab === "worldview") return isWorldviewRelatedNode(n);
  if (tab === "characters") return n.kind === "character";
  if (tab === "story-rules") {
    const s = knowledgeSlot(n);
    return isStoryRulesSlot(s) || isStoryRulesFanSlot(s);
  }
  if (tab === "plots") return n.kind === "side_plot";
  if (tab === "tree") return true;
  return n.kind === "volume" || n.kind === "chapter";
}

async function applyTabDefaultSelection(force = false) {
  const tr = tree.value;
  if (!tr) return;
  const tab = workspaceTab.value;
  if (!force && selected.value && selectionFitsTab(selected.value, tab)) return;
  const root = tr.nodes.find((n) => n.kind === "novel");
  if (tab === "book") {
    if (root) await selectNode(root);
    return;
  }
  if (tab === "worldview") {
    const first = WORLDVIEW_FAN_SLOTS.map((d) =>
      tr.nodes.find((n) => n.kind === "knowledge" && knowledgeSlot(n) === d.slot),
    ).find((n): n is TreeNode => !!n);
    if (first) await selectNode(first);
    else if (root) await selectNode(root);
    return;
  }
  if (tab === "characters") {
    const c = tr.nodes.find((n) => n.kind === "character");
    if (c) await selectNode(c);
    else if (root) await selectNode(root);
    return;
  }
  if (tab === "story-rules") {
    const hub = findStoryRulesNode(tr);
    if (hub) await selectNode(hub);
    else if (root) await selectNode(root);
    return;
  }
  if (tab === "plots") {
    const p = tr.nodes.find((n) => n.kind === "side_plot");
    if (p) await selectNode(p);
    else {
      selected.value = null;
      void api.setWorkspaceSelection(props.id, null);
    }
    return;
  }
  if (tab === "tree") {
    if (selected.value) return;
    if (root) await selectNode(root);
    return;
  }
  const ch = tr.nodes.find((n) => n.kind === "chapter");
  const vol = tr.nodes.find((n) => n.kind === "volume");
  if (ch) await selectNode(ch);
  else if (vol) await selectNode(vol);
  else if (root) await selectNode(root);
}

async function openTreeNode(id: string) {
  if (!id) return;
  const n = tree.value?.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

watch(workspaceTab, () => {
  closeRelationEditor();
  void applyTabDefaultSelection();
});

/** 选中章节后始终进入全文编辑（无 Markdown 预览模式） */
function enterChapterBodyEdit() {
  if (!selected.value || selected.value.kind !== "chapter") {
    leaveChapterBodyEdit();
    return;
  }
  closeBodySuggest();
  bodyDraft.value = stripChapterMeta(chapterMd.value);
  bodyEditing.value = true;
  bodyAutosaved.value = false;
  lastSavedBody = bodyDraft.value;
  nextTick(() => bodyEditor.value?.scrollToTop());
  if (bodySuggestOn.value) scheduleBodySuggest();
}

function leaveChapterBodyEdit() {
  bodyEditing.value = false;
  bodyDraft.value = "";
  bodyAutosaved.value = false;
  lastSavedBody = "";
  pendingParaRewrite.value = null;
  closeBodySuggest();
  void stopChapterTts();
  clearBodyDiff();
}

function clearBodyDiff() {
  bodyView.value = "edit";
  bodyDiffBefore.value = "";
  bodyDiffAfter.value = "";
  bodyDiffOrigin.value = null;
}

function pickBodyDiff(index: number, side: "old" | "new") {
  const { before, after } = applyDiffPick(bodyDiffBlocks.value, index, side);
  bodyDiffBefore.value = before;
  bodyDiffAfter.value = after;
  bodyDraft.value = after;
  if (!diffLines(before, after).some((h) => h.type !== "eq")) {
    clearBodyDiff();
  }
  void persistChapterBody();
}

function queueChatChapterBody(nodeId: string, beforeRaw: string) {
  const incoming = stripChapterMeta(beforeRaw);
  const origin =
    bodyDiffOrigin.value?.nodeId === nodeId
      ? bodyDiffOrigin.value.before
      : incoming;
  if (!contentChangePending || contentChangePending.nodeId !== nodeId) {
    contentChangePending = { nodeId, before: origin };
  }
  if (contentChangeFlush != null) clearTimeout(contentChangeFlush);
  contentChangeFlush = setTimeout(() => {
    const p = contentChangePending;
    contentChangePending = null;
    contentChangeFlush = null;
    if (p) void applyChatChapterBody(p.nodeId, p.before);
  }, 100);
}

async function applyChatChapterBody(nodeId: string, beforeRaw: string) {
  const gen = ++chapterContentGen;
  if (
    selected.value?.kind === "chapter" &&
    selected.value.id !== nodeId &&
    bodyDraft.value !== lastSavedBody
  ) {
    await persistChapterBody();
    if (gen !== chapterContentGen) return;
  }
  await reloadTreeFromDisk();
  if (gen !== chapterContentGen) return;
  const n = tree.value?.nodes.find((x) => x.id === nodeId);
  if (!n || n.kind !== "chapter") return;
  const afterRaw = await api.getChapter(props.id, nodeId);
  if (gen !== chapterContentGen) return;
  const before = stripChapterMeta(
    bodyDiffOrigin.value?.nodeId === nodeId
      ? bodyDiffOrigin.value.before
      : beforeRaw,
  );
  const after = stripChapterMeta(afterRaw);
  if (selected.value?.id !== nodeId) {
    await selectNode(n);
    if (gen !== chapterContentGen) return;
  } else {
    chapterMd.value = afterRaw;
    bodyDraft.value = after;
    lastSavedBody = after;
    bodyAutosaved.value = false;
  }
  workspaceTab.value = "manuscript";
  if (before === after) return;
  bodyDiffBefore.value = before;
  bodyDiffAfter.value = after;
  bodyView.value = "diff";
}

const bodyLines = computed(() => splitBodyLines(bodyDraft.value));

/** 与后端 count_words 一致：去掉空白后的字符数。 */
const bodyWordCount = computed(() =>
  countWords(stripChapterMeta(bodyEditing.value ? bodyDraft.value : chapterMd.value)),
);

const bodyNouns = computed(() => {
  const names: string[] = [];
  for (const n of tree.value?.nodes ?? []) {
    if (n.kind !== "character") continue;
    const label = n.label?.trim();
    if (label) names.push(label);
  }
  return [...new Set(names)];
});

async function persistChapterBody() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  while (bodySaveBusy.value) {
    await new Promise((r) => setTimeout(r, 20));
  }
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
  if (!bodySuggestOn.value) return;
  if (bodySuggestBusy.value) {
    bodySuggestGen += 1;
    void api.chatCancel(props.id).catch(() => {});
  }
  scheduleBodySuggest();
});


function openParaRewrite(index: number) {
  const line = bodyLines.value[index];
  if (!line?.trim() || paraRewriteBusy.value) return;
  if (bodySuggestBusy.value) {
    bodySuggestGen += 1;
    void api.chatCancel(props.id).catch(() => {});
    bodySuggestBusy.value = false;
  }
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
    pendingParaRewrite.value = null;
    paraRewriteNote.value = "";
    await nextTick();
    if (!bodyEditor.value?.replaceLineAndSelect(pending.index, out)) {
      bodyDraft.value = replaceBodyLine(bodyDraft.value, pending.index, out);
    }
    await persistChapterBody();
  } catch (e) {
    paraRewriteError.value = String(e);
  } finally {
    paraRewriteBusy.value = false;
  }
}

function closeBodySuggest() {
  bodySuggestGen += 1;
  if (bodySuggestBusy.value) {
    void api.chatCancel(props.id).catch(() => {});
  }
  bodySuggestBusy.value = false;
  bodySuggestError.value = "";
  bodySuggestItems.value = [];
}

function toggleBodySuggest() {
  bodySuggestOn.value = !bodySuggestOn.value;
  if (!bodySuggestOn.value) {
    closeBodySuggest();
    return;
  }
  scheduleBodySuggest();
}

const scheduleBodySuggest = useDebounceFn(() => {
  if (!bodySuggestOn.value) return;
  void requestBodySuggest();
}, 200);

async function requestBodySuggest() {
  if (!bodySuggestOn.value || !bodyEditing.value || paraRewriteBusy.value || pendingParaRewrite.value) return;
  if (!selected.value || selected.value.kind !== "chapter") return;
  bodySuggestInsertAt = bodyEditor.value?.getCursor() ?? bodyDraft.value.length;
  const { current, prevParagraph } = bodySuggestContext(bodyDraft.value, bodySuggestInsertAt);
  if (bodySuggestBusy.value) {
    bodySuggestGen += 1;
    void api.chatCancel(props.id).catch(() => {});
  }
  const gen = ++bodySuggestGen;
  const nodeId = selected.value.id;
  bodySuggestBusy.value = true;
  bodySuggestError.value = "";
  try {
    const items = await api.suggestBodyNext(props.id, nodeId, current, prevParagraph);
    if (gen !== bodySuggestGen) return;
    bodySuggestItems.value = items;
  } catch (e) {
    if (gen !== bodySuggestGen) return;
    bodySuggestError.value = String(e);
  } finally {
    if (gen === bodySuggestGen) bodySuggestBusy.value = false;
  }
}

function applyBodySuggest(item: string) {
  if (!item.trim()) return;
  const { text, cursor } = insertBodySuggestion(bodyDraft.value, bodySuggestInsertAt, item);
  bodyDraft.value = text;
  bodySuggestInsertAt = cursor;
  nextTick(() => {
    bodyEditor.value?.setSelection(cursor);
  });
}

const CHAPTER_STEPS: Record<string, MessageKey> = {
  confirm_model: "workspace.taskStepConfirmModel",
  context: "workspace.taskStepContext",
  detailed_outline: "workspace.taskStepDetailedOutline",
  writing: "workspace.taskStepWriting",
  refining: "workspace.taskStepRefining",
  check_beats: "workspace.taskStepCheckBeats",
  check_lore: "workspace.taskStepCheckLore",
  repair_land: "workspace.taskStepRepairLand",
  repair_beats: "workspace.taskStepCheckBeats",
  repair_length: "workspace.taskStepRepairLength",
  memory: "workspace.taskStepMemory",
  planning: "workspace.taskStepPlanning",
  gen_plots: "workspace.taskStepGenPlots",
  saving: "workspace.taskStepSaving",
};

function chapterStepLabel(step: string, detail?: string): string {
  const key = CHAPTER_STEPS[step];
  if (!key) return step;
  if (step === "confirm_model") {
    return t(key, { model: detail?.trim() || "…" });
  }
  return t(key);
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

function beginChapterTask(step: string, index: number, total: number, detail?: string) {
  stopChapterTick();
  chapterStepStartedAt = performance.now();
  chapterProgress.value = { step, index, total, detail };
  chapterTokens.value = null;
  chapterStepTimings.value = [{ step, ms: 0, done: false, detail }];
  startChapterTick();
}

function applyChapterProgress(
  step: string,
  index: number,
  total: number,
  detail?: string,
) {
  const now = performance.now();
  const list = [...chapterStepTimings.value];
  if (list.length) {
    const last = list[list.length - 1];
    if (!last.done && last.step === step) {
      chapterProgress.value = { step, index, total, detail: detail ?? last.detail };
      if (detail) last.detail = detail;
      return;
    }
    if (!last.done) {
      last.ms = Math.round(now - chapterStepStartedAt);
      last.done = true;
    }
  }
  list.push({ step, ms: 0, done: false, detail });
  chapterStepTimings.value = list;
  chapterStepStartedAt = now;
  chapterProgress.value = { step, index, total, detail };
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
  return chapterStepLabel(p.step, p.detail);
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
    label: chapterStepLabel(s.step, s.detail),
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
    (v !== "plan-next" &&
      v !== "gen-plots" &&
      v !== "regen-memory" &&
      v !== "gen-cards" &&
      v !== "consolidate-plots")
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
    detail?: string;
  }>("chapter-progress", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    applyChapterProgress(
      ev.payload.step,
      ev.payload.index,
      ev.payload.total,
      ev.payload.detail,
    );
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
  unlistenChapterContent = await listen<{
    novelId: string;
    nodeId: string;
    before?: string;
  }>("chapter-content-changed", (ev) => {
    if (ev.payload.novelId !== props.id) return;
    void queueChatChapterBody(ev.payload.nodeId, ev.payload.before ?? "");
  });
  unlistenChapterTts = await listen<{ status: string }>("chapter-tts-status", (ev) => {
    if (ev.payload.status === "playing") {
      bodyTtsDownload.value = null;
      bodyTts.value = "playing";
    }
    if (ev.payload.status === "idle") bodyTts.value = "idle";
  });
  unlistenChapterTtsDownload = await listen<TtsDownloadProgress>("chapter-tts-download", (ev) => {
    const p = ev.payload;
    if (p?.phase === "done") {
      bodyTtsDownload.value = null;
      return;
    }
    if (bodyTts.value === "idle") return;
    bodyTtsDownload.value = p ?? null;
  });
  unlistenChapterTtsChunk = await listen<{ start: number; end: number }>("chapter-tts-chunk", (ev) => {
    if (bodyTts.value === "idle") return;
    selectBodyTtsRange(ev.payload.start, ev.payload.end);
  });
});
onUnmounted(() => {
  unlistenChapterProgress?.();
  unlistenChapterProgress = null;
  unlistenChapterTokens?.();
  unlistenChapterTokens = null;
  unlistenTreeChanged?.();
  unlistenTreeChanged = null;
  unlistenChapterContent?.();
  unlistenChapterContent = null;
  unlistenChapterTts?.();
  unlistenChapterTts = null;
  unlistenChapterTtsDownload?.();
  unlistenChapterTtsDownload = null;
  unlistenChapterTtsChunk?.();
  unlistenChapterTtsChunk = null;
  if (contentChangeFlush != null) {
    clearTimeout(contentChangeFlush);
    contentChangeFlush = null;
  }
  contentChangePending = null;
  stopChapterTick();
  void stopChapterTts();
  // Only clear if still this novel — late clear must not wipe the next workspace.
  void api.setWorkspaceSelection(props.id, null);
});
watch(
  () => props.id,
  async (_id, prev) => {
    if (prev) void api.setWorkspaceSelection(prev, null);
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

async function loadChapterShots(nodeId: string, showBusy = true) {
  if (showBusy) shotsBusy.value = true;
  try {
    shots.value = await api.getChapterShots(props.id, nodeId);
  } catch (e) {
    if (showBusy) shotsError.value = String(e);
    else shots.value = [];
  } finally {
    if (showBusy) shotsBusy.value = false;
  }
}

async function openChapterShots() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  shotsPanelOpen.value = true;
  shotsError.value = "";
  shotsNotice.value = "";
  await loadChapterShots(selected.value.id, true);
}

function closeChapterShots() {
  if (busy.value === "split-shots" || busy.value === "shot-prompts" || busy.value === "submit-comfy") {
    return;
  }
  shotsPanelOpen.value = false;
  pendingShotSplit.value = null;
}

function requestSplitShots() {
  if (shots.value.length) {
    pendingShotSplit.value = 1;
    return;
  }
  void runSplitShots();
}

async function runSplitShots() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value) return;
  pendingShotSplit.value = null;
  busy.value = "split-shots";
  shotsError.value = "";
  shotsNotice.value = "";
  try {
    shots.value = await api.splitChapterShots(props.id, selected.value.id);
  } catch (e) {
    shotsError.value = String(e);
  } finally {
    busy.value = "";
  }
}

async function runShotPrompts() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value) return;
  busy.value = "shot-prompts";
  shotsError.value = "";
  shotsNotice.value = "";
  try {
    shots.value = await api.generateShotComfyPrompts(props.id, selected.value.id);
  } catch (e) {
    shotsError.value = String(e);
  } finally {
    busy.value = "";
  }
}

async function persistShots() {
  if (!selected.value || selected.value.kind !== "chapter") return;
  try {
    shots.value = await api.setChapterShots(props.id, selected.value.id, shots.value);
  } catch (e) {
    shotsError.value = String(e);
  }
}

function addShot() {
  shots.value.push({
    id: "",
    order: shots.value.length + 1,
    action: "",
    camera: "",
    dialogue: "",
    duration_sec: 8,
    comfy_prompt: "",
  });
  void persistShots();
}

function removeShot(i: number) {
  shots.value.splice(i, 1);
  void persistShots();
}

async function runSubmitComfy() {
  if (!selected.value || selected.value.kind !== "chapter" || !!busy.value) return;
  await persistShots();
  busy.value = "submit-comfy";
  shotsError.value = "";
  shotsNotice.value = "";
  try {
    const r = await api.submitChapterShotsComfyui(props.id, selected.value.id);
    shotsNotice.value = t("workspace.shotsSubmitted", {
      n: String(r.queued),
      mode: r.mode,
      url: r.url,
    });
  } catch (e) {
    shotsError.value = String(e);
  } finally {
    busy.value = "";
  }
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


async function stopChapter() {
  if (!chapterBusy.value) return;
  try {
    await api.chatCancel(props.id);
  } catch {
    /* ignore */
  }
}

let chapterTtsReq = 0;

async function stopChapterTts() {
  chapterTtsReq += 1;
  bodyTts.value = "idle";
  bodyTtsDownload.value = null;
  try {
    await api.stopChapterTts();
  } catch {
    /* ignore */
  }
}

function formatTtsMb(n: number) {
  return (n / (1024 * 1024)).toFixed(1);
}

const ttsDownloadLabel = computed(() => {
  const p = bodyTtsDownload.value;
  if (!p?.phase) return "";
  if (p.phase === "download") return t("workspace.readAloudModelDownloading");
  if (p.phase === "load") return t("workspace.readAloudModelLoading");
  return "";
});

const ttsDownloadPercent = computed(() =>
  Math.max(0, Math.min(100, Math.round(bodyTtsDownload.value?.percent ?? 0))),
);

const ttsDownloadDetail = computed(() => {
  const p = bodyTtsDownload.value;
  if (!p || p.phase !== "download" || !p.file) return "";
  const file = p.file.split("/").pop() || p.file;
  if (p.bytesTotal && p.bytesTotal > 0) {
    return t("workspace.readAloudModelBytes", {
      file,
      done: formatTtsMb(p.bytesDownloaded ?? 0),
      total: formatTtsMb(p.bytesTotal),
    });
  }
  return t("workspace.readAloudModelFile", {
    file,
    index: p.fileIndex ?? 0,
    total: p.fileTotal ?? 0,
  });
});

function selectBodyTtsRange(start: number, end: number) {
  if (!bodyEditing.value || !bodyEditor.value) return;
  const len = bodyDraft.value.length;
  const s = Math.max(0, Math.min(Math.floor(start), len));
  const e = Math.max(s, Math.min(Math.floor(end), len));
  if (e <= s) return;
  bodyEditor.value.setSelection(s, e);
}

async function toggleChapterTts() {
  if (bodyTts.value !== "idle") {
    await stopChapterTts();
    return;
  }
  const md = bodyEditing.value ? bodyDraft.value : stripChapterMeta(chapterMd.value);
  if (!md.trim()) {
    notice.value = t("workspace.readAloudEmpty");
    return;
  }
  const req = ++chapterTtsReq;
  bodyTts.value = "loading";
  try {
    await api.playChapterTts(md, Number(ttsSpeed.value) || 1);
  } catch (e) {
    if (req === chapterTtsReq) notice.value = String(e);
  } finally {
    if (req === chapterTtsReq) {
      bodyTts.value = "idle";
      bodyTtsDownload.value = null;
    }
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
  return emptyCharacterCard();
}

const characterLawOptions = computed(() => {
  const tr = tree.value;
  if (!tr) return [] as string[];
  const core = tr.nodes.find(
    (n) => n.kind === "knowledge" && (n.knowledge?.slot ?? "").trim() === "wv_core_laws",
  );
  if (!core) return [];
  const names = axiomsFromLinkedCards(tr, core.id)
    .map((a) => a.name.trim())
    .filter(Boolean);
  const premise = (core.knowledge?.core_laws?.premise ?? "").trim();
  if (premise) names.unshift(premise);
  return [...new Set(names)];
});

const otherCharacterNames = computed(() => {
  const selId = selected.value?.id;
  return (tree.value?.nodes ?? [])
    .filter((n) => n.kind === "character" && n.id !== selId)
    .map((n) => n.label.trim())
    .filter(Boolean);
});

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
    history_culture: null,
    world_religion: null,
    major_event: null,
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

function onNodeClick(ev: { node: { data: FlowCanvasNode["data"] } }) {
  const data = ev.node.data;
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
    if (charId) {
      host.linked_character_ids = (host.linked_character_ids ?? []).filter((id) => id !== charId);
    }
  }
  if (e.kind === "side_plot") {
    const plotId = src.kind === "side_plot" ? src.id : tgt.kind === "side_plot" ? tgt.id : null;
    const host = src.kind === "side_plot" ? tgt : src;
    if (plotId) {
      host.linked_side_plot_ids = (host.linked_side_plot_ids ?? []).filter((id) => id !== plotId);
    }
  }
  if (e.kind === "knowledge") {
    const wp = writePromptsLinkHost(src, tgt);
    const kid = wp
      ? wp.card.kind === "knowledge"
        ? wp.card
        : null
      : src.kind === "knowledge"
        ? src
        : tgt.kind === "knowledge"
          ? tgt
          : null;
    const host = wp ? wp.host : src.kind === "knowledge" ? tgt : src;
    if (kid) {
      host.linked_knowledge_ids = (host.linked_knowledge_ids ?? []).filter((id) => id !== kid.id);
    }
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
    host.linked_character_ids = host.linked_character_ids ?? [];
    if (char && !host.linked_character_ids.includes(char.id)) {
      host.linked_character_ids.push(char.id);
    }
  }
  if (e.kind === "side_plot") {
    const plot = src.kind === "side_plot" ? src : tgt.kind === "side_plot" ? tgt : null;
    const host = src.kind === "side_plot" ? tgt : src;
    if (!plot) return;
    if (host.kind === "chapter") {
      const inherited = new Set(
        chapterInheritedPlotIds(host.id, tree.value.nodes, tree.value.edges),
      );
      if (inherited.has(plot.id)) return;
    } else if (host.kind === "volume") {
      const rootSet = new Set(rootLinkedPlotIds(tree.value.nodes, tree.value.edges));
      if (rootSet.has(plot.id)) return;
    }
    host.linked_side_plot_ids = host.linked_side_plot_ids ?? [];
    if (!host.linked_side_plot_ids.includes(plot.id)) {
      host.linked_side_plot_ids.push(plot.id);
    }
  }
  if (e.kind === "knowledge") {
    const wp = writePromptsLinkHost(src, tgt);
    const k = wp
      ? wp.card.kind === "knowledge"
        ? wp.card
        : null
      : src.kind === "knowledge"
        ? src
        : tgt.kind === "knowledge"
          ? tgt
          : null;
    const host = wp ? wp.host : src.kind === "knowledge" ? tgt : src;
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

async function onConnect(conn: FlowConnect) {
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
    notice.value = isWritePromptsRootEdge(tree.value.nodes, te)
      ? t("workspace.writePrompts.linkLocked")
      : t("workspace.wv.linkLocked");
    return;
  }
  if (relationEdgeId.value === te.id) closeRelationEditor();
  unlinkEdgeRefs(te);
  tree.value.edges = tree.value.edges.filter((e) => e.id !== te.id);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  notice.value = t("workspace.edgeRemoved");
}

async function onNodeDragStop(ev: { node: { id: string; position: { x: number; y: number } } }) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === ev.node.id);
  if (!n) return;
  n.position = { x: ev.node.position.x, y: ev.node.position.y };
  tree.value = await persistTree(tree.value);
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
      detailed_outline: [],
      character: null,
      knowledge: null,
      side_plot: null,
      volume: emptyVolume(),
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
      detailed_outline: [],
      character: null,
      knowledge: null,
      side_plot: null,
      volume: null,
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
      workspaceTab.value === "characters"
        ? root
        : selected.value &&
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
      detailed_outline: [],
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
      workspaceTab.value === "book"
        ? root
        : selected.value &&
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
      detailed_outline: [],
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
    let host = root;
    if (workspaceTab.value !== "plots") {
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
    }
    const plots = tree.value.nodes.filter((n) => n.kind === "side_plot").length;
    node = {
      id,
      kind,
      label: t("workspace.newPlot"),
      outline: "",
      detailed_outline: [],
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
const selectedIsHistoryCulture = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    knowledgeSlot(selected.value) === "wv_history_culture",
);
const selectedIsWorldReligion = computed(
  () => selected.value?.kind === "knowledge" && isReligionSlot(knowledgeSlot(selected.value)),
);
const selectedIsMajorEvent = computed(
  () => selected.value?.kind === "knowledge" && isMajorEventSlot(knowledgeSlot(selected.value)),
);
const selectedWorldviewFanSlot = computed(() => {
  if (!selected.value || selected.value.kind !== "knowledge") return "";
  const slot = knowledgeSlot(selected.value);
  return isWorldviewFanSlot(slot) ? slot : "";
});
const selectedIsStoryRules = computed(
  () =>
    selected.value?.kind === "knowledge" && isStoryRulesSlot(knowledgeSlot(selected.value)),
);
const selectedIsWritePrompts = computed(
  () =>
    selected.value?.kind === "knowledge" && isWritePromptsSlot(knowledgeSlot(selected.value)),
);
const writePromptPanels = computed(() => {
  const tr = tree.value;
  return [
    {
      side: "generate" as const,
      titleKey: "workspace.writePrompts.handleGenerate" as const,
      cards: tr ? writePromptCardsForSide(tr, "generate") : [],
      chipClass: "bg-teal-100 text-teal-900 dark:bg-teal-950/40 dark:text-teal-100",
      plusClass:
        "border-teal-400 text-teal-800 hover:bg-teal-50 dark:hover:bg-teal-950/40",
    },
    {
      side: "refine" as const,
      titleKey: "workspace.writePrompts.handleRefine" as const,
      cards: tr ? writePromptCardsForSide(tr, "refine") : [],
      chipClass: "bg-indigo-100 text-indigo-900 dark:bg-indigo-950/40 dark:text-indigo-100",
      plusClass:
        "border-indigo-400 text-indigo-800 hover:bg-indigo-50 dark:hover:bg-indigo-950/40",
    },
  ];
});
const selectedIsStoryRulesBlock = computed(
  () =>
    selected.value?.kind === "knowledge" &&
    !!storyRulesBlockSlotOf(selected.value),
);
const selectedIsPlainKnowledge = computed(
  () =>
    selectedIsKnowledge.value &&
    !selectedIsCoreLaws.value &&
    !selectedIsWorldAxiom.value &&
    !selectedIsKeyLocation.value &&
    !selectedIsSocialPower.value &&
    !selectedIsWorldRace.value &&
    !selectedIsMajorFaction.value &&
    !selectedIsSpatiotemporal.value &&
    !selectedIsExistence.value &&
    !selectedIsInfoFlow.value &&
    !selectedIsHistoryCulture.value &&
    !selectedIsWorldReligion.value &&
    !selectedIsMajorEvent.value &&
    !selectedIsStoryRules.value &&
    !selectedIsStoryRulesBlock.value &&
    !selectedIsWritePrompts.value,
);
const selectedStoryRulesBlockSlot = computed((): StoryRulesBlockSlot | null => {
  if (!selected.value || !selectedIsStoryRulesBlock.value) return null;
  return storyRulesBlockSlotOf(selected.value) as StoryRulesBlockSlot;
});
const storyRulesBlockLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsStoryRules.value) return [];
  return linkedStoryRulesBlocks(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    slot: knowledgeSlot(n),
    title: knowledgeNodeLabel(n, t),
  }));
});
const selectedIsNovel = computed(() => selected.value?.kind === "novel");

const sideNavItems = computed(() => {
  if (!tree.value) return [];
  if (workspaceTab.value === "book") return buildBookNavItems(tree.value, t);
  if (workspaceTab.value === "worldview") return buildWorldviewNavItems(tree.value, t);
  if (workspaceTab.value === "characters") return buildCharacterNavItems(tree.value, t);
  if (workspaceTab.value === "story-rules") return buildStoryRulesNavItems(tree.value, t);
  if (workspaceTab.value === "plots") return buildPlotNavItems(tree.value, t);
  return [];
});

type ManuscriptRow = {
  id: string;
  label: string;
  kind: "volume" | "chapter";
  indent: number;
};

const manuscriptRows = computed((): ManuscriptRow[] => {
  const tr = tree.value;
  if (!tr) return [];
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  const volumes = sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "volume"));
  const chapters = sortNodesByCanvasPosition(tr.nodes.filter((n) => n.kind === "chapter"));
  const rows: ManuscriptRow[] = [];
  const listedCh = new Set<string>();
  for (const vol of volumes) {
    rows.push({
      id: vol.id,
      label: vol.label?.trim() || t("workspace.volumeBadge"),
      kind: "volume",
      indent: 0,
    });
    for (const cid of sortNodesByCanvasPosition(
      volumeChapterIds(vol.id, tr.nodes, tr.edges)
        .map((id) => byId.get(id))
        .filter((n): n is TreeNode => !!n),
    )) {
      listedCh.add(cid.id);
      rows.push({
        id: cid.id,
        label: cid.label?.trim() || t("workspace.newChapter"),
        kind: "chapter",
        indent: 1,
      });
    }
  }
  for (const ch of chapters) {
    if (listedCh.has(ch.id)) continue;
    rows.push({
      id: ch.id,
      label: ch.label?.trim() || t("workspace.newChapter"),
      kind: "chapter",
      indent: 0,
    });
  }
  return rows;
});

function openWordStats() {
  const n = manuscriptRows.value.filter((r) => r.kind === "chapter").length;
  wordStatsOpen.value = true;
  wordStatsMode.value = "all";
  wordStatsFrom.value = 1;
  wordStatsTo.value = Math.max(1, n);
  wordStatsResult.value = null;
  wordStatsError.value = "";
}

function closeWordStats() {
  if (wordStatsBusy.value) return;
  wordStatsOpen.value = false;
}

async function runWordStats() {
  if (wordStatsBusy.value) return;
  const chapters = manuscriptRows.value.filter((r) => r.kind === "chapter");
  if (!chapters.length) {
    wordStatsError.value = t("workspace.wordStatsEmpty");
    wordStatsResult.value = null;
    return;
  }
  wordStatsBusy.value = true;
  wordStatsError.value = "";
  wordStatsResult.value = null;
  try {
    await persistChapterBody();
    const [lo, hi] =
      wordStatsMode.value === "all"
        ? chapterSpan(chapters.length, null, null)
        : chapterSpan(chapters.length, Number(wordStatsFrom.value), Number(wordStatsTo.value));
    const slice = chapters.slice(lo, hi);
    const items: { label: string; words: number }[] = [];
    let total = 0;
    for (const row of slice) {
      let words = 0;
      try {
        const body = await api.getChapter(props.id, row.id);
        words = countWords(stripChapterMeta(body));
      } catch {
        words = 0;
      }
      items.push({ label: row.label, words });
      total += words;
    }
    wordStatsResult.value = {
      from: lo + 1,
      to: hi,
      total,
      items,
    };
  } catch (e) {
    wordStatsError.value = String(e);
  } finally {
    wordStatsBusy.value = false;
  }
}

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

const historyCultureReligionLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return [];
  return linkedReligionCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title: n.knowledge?.world_religion?.name?.trim() || n.label || "",
  }));
});

const historyCultureEventLinks = computed(() => {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return [];
  return linkedMajorEventCards(tree.value, selected.value.id).map((n) => ({
    id: n.id,
    title: n.knowledge?.major_event?.title?.trim() || n.label || "",
  }));
});

const historyCultureLinkedReligions = computed((): WorldReligion[] => {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return [];
  return religionsFromLinkedCards(tree.value, selected.value.id);
});

const historyCultureLinkedEvents = computed((): MajorEvent[] => {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return [];
  return majorEventsFromLinkedCards(tree.value, selected.value.id);
});
const selectedIsVolume = computed(() => selected.value?.kind === "volume");

const rootWorldviewComplete = computed(() =>
  tree.value ? worldviewComplete(tree.value) : false,
);
const worldviewBusy = ref(false);
const worldviewChatOpen = ref(false);
const worldviewChatSlot = ref<string | null>(null);
const storyRulesChatOpen = ref(false);

const titleI18n = (key: string) => t(key as Parameters<typeof t>[0]);

async function openWorldviewChat(slot?: string | null) {
  if (!tree.value || worldviewBusy.value) return;
  const slotId = typeof slot === "string" && slot.trim() ? slot.trim() : null;
  worldviewBusy.value = true;
  try {
    const before = missingWorldviewSlots(tree.value).length;
    ensureWorldviewCards(tree.value, titleI18n);
    ensureStoryRulesFanCards(tree.value, titleI18n);
    migrateInlineAxiomsToCards(tree.value);
    migrateInlineLocationsToCards(tree.value);
    migrateInlineSocialPowerToCards(tree.value);
    migrateInlineHistoryCultureToCards(tree.value);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
    if (before > 0) {
      await nextTick();
      void fitView({ padding: 0.18, duration: 280 });
    }
    worldviewChatSlot.value = slotId;
    worldviewChatOpen.value = true;
  } catch (e) {
    notice.value = String(e);
  } finally {
    worldviewBusy.value = false;
  }
}

provide("novework.openWorldviewSlotChat", (slot: string) => {
  void openWorldviewChat(slot);
});

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

async function autoLayout() {
  if (!tree.value) return;
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await nextTick();
  void fitView({ padding: 0.18, duration: 280 });
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
  const historyHost =
    tree.value && selected.value && isHistoryCultureChildSlot(knowledgeSlot(selected.value))
      ? historyCultureHostId(tree.value, id)
      : tree.value
        ? historyCultureHostId(tree.value, id)
        : null;
  try {
    tree.value = await api.deleteTreeCard(props.id, id);
    pruneTreeRefs(tree.value);
    if (axiomHost) {
      syncCoreLawsExtracted(tree.value, axiomHost);
      tree.value = await persistTree(tree.value);
    } else if (locationHost) {
      syncSpatiotemporalExtracted(tree.value, locationHost);
      tree.value = await persistTree(tree.value);
    } else if (socialHost) {
      syncSocialPowerExtracted(tree.value, socialHost);
      tree.value = await persistTree(tree.value);
    } else if (historyHost) {
      syncHistoryCultureExtracted(tree.value, historyHost);
      tree.value = await persistTree(tree.value);
    }
    syncFlowFromTree();

    pendingCardDelete.value = null;
    const prefer =
      (axiomHost && tree.value.nodes.find((n) => n.id === axiomHost)) ||
      (locationHost && tree.value.nodes.find((n) => n.id === locationHost)) ||
      (socialHost && tree.value.nodes.find((n) => n.id === socialHost)) ||
      (historyHost && tree.value.nodes.find((n) => n.id === historyHost)) ||
      tree.value.nodes.find((n) => n.kind === "novel");
    if (prefer) await selectNode(prefer);
    else {
      selected.value = null;
      void api.setWorkspaceSelection(props.id, null);
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

async function persistSelectedHistoryCulture(payload: {
  historyCulture: HistoryCultureData;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  applyWorldviewFanLabel(selected.value, "wv_history_culture");
  k.history_culture = payload.historyCulture;
  k.extracted = payload.extracted;
  k.slot = "wv_history_culture";
  await persistSelectedKnowledge();
}

async function persistSelectedWorldReligion(payload: {
  name: string;
  worldReligion: WorldReligion;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.world_religion = normalizeReligion(payload.worldReligion);
  k.extracted = payload.extracted;
  k.slot = "wv_religion";
  const hostId = historyCultureHostId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncHistoryCultureExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function persistSelectedMajorEvent(payload: {
  name: string;
  majorEvent: MajorEvent;
  extracted: string;
}) {
  if (!tree.value || !selected.value || selected.value.kind !== "knowledge") return;
  const k = ensureKnowledgePayload();
  if (!k) return;
  selected.value.label = payload.name;
  k.major_event = normalizeMajorEvent(payload.majorEvent);
  k.extracted = payload.extracted;
  k.slot = "wv_major_event";
  const hostId = historyCultureHostId(tree.value, selected.value.id);
  await persistSelectedKnowledge();
  if (hostId && tree.value) {
    syncHistoryCultureExtracted(tree.value, hostId);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
}

async function addHistoryCultureReligion() {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return;
  const label = t("workspace.hc.religionUntitled");
  const node = createReligionCard(tree.value, selected.value.id, label);
  if (!node) return;
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
}

async function addHistoryCultureEvent() {
  if (!tree.value || !selected.value || !selectedIsHistoryCulture.value) return;
  const label = t("workspace.hc.eventUntitled");
  const node = createMajorEventCard(tree.value, selected.value.id, label);
  if (!node) return;
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  await selectNode(node);
}

async function openHistoryCultureReligion(id: string) {
  const n = tree.value?.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function openHistoryCultureEvent(id: string) {
  const n = tree.value?.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
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

async function onStoryRulesChatApplied() {
  if (!tree.value) return;
  try {
    let tr = await api.getTree(props.id);
    tr = await maybeMigrateWorldview(tr);
    tree.value = tr;
    if (selected.value) {
      const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
      if (n) selected.value = n;
    }
    syncFlowFromTree();
    await nextTick();
    void fitView({ padding: 0.18, duration: 280 });
  } catch (e) {
    notice.value = String(e);
  }
}

function openStoryRulesChat() {
  storyRulesChatOpen.value = true;
}

async function openStoryRulesBlock(id: string) {
  if (!tree.value) return;
  const n = tree.value.nodes.find((x) => x.id === id);
  if (n) await selectNode(n);
}

async function persistSelectedVolume(payload: { volume: VolumeData; nodeId?: string }) {
  if (!tree.value) return;
  const id = payload.nodeId ?? selected.value?.id;
  if (!id) return;
  const n = tree.value.nodes.find((x) => x.id === id && x.kind === "volume");
  if (!n) return;
  const vol = normalizeVolume(payload.volume);
  n.volume = vol;
  if (selected.value?.id === id) selected.value.volume = vol;
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
}

async function persistSelectedStoryRulesBlock(payload: Record<string, unknown>) {
  if (!tree.value || !selected.value || !selectedStoryRulesBlockSlot.value) return;
  const slot = selectedStoryRulesBlockSlot.value;
  const k = ensureKnowledgePayload();
  if (!k) return;
  const data = normalizeStoryRulesBlock(slot, payload);
  const extracted = formatStoryRulesBlockExtracted(slot, data);
  k.slot = slot;
  k.surface_setting = null;
  k.story_engine = null;
  k.fulfillment_system = null;
  k.constraint_redlines = null;
  if (slot === "sr_surface_setting") k.surface_setting = data as SurfaceSettingData;
  else if (slot === "sr_story_engine") k.story_engine = data as StoryEngineData;
  else if (slot === "sr_fulfillment_system") k.fulfillment_system = data as FulfillmentSystemData;
  else k.constraint_redlines = data as ConstraintRedlinesData;
  k.extracted = extracted;
  selected.value.outline = extracted.slice(0, 200);
  await persistSelectedKnowledge();
  const rules = findStoryRulesNode(tree.value);
  if (rules) {
    syncStoryRulesParentExtracted(tree.value, rules.id, titleI18n);
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
  }
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

const existingKnowledgeTitles = computed(() => {
  const titles = new Set<string>();
  for (const n of tree.value?.nodes ?? []) {
    if (n.kind !== "knowledge") continue;
    const title = n.label.trim();
    if (title) titles.add(title);
  }
  return titles;
});

function publicCardAlreadyOnTree(card: PublicKnowledgeCard): boolean {
  const title = card.title.trim();
  return title !== "" && existingKnowledgeTitles.value.has(title);
}

async function openPublicPick(side?: WritePromptKind) {
  publicPickWriteSide.value = side === "generate" || side === "refine" ? side : null;
  publicPickFilter.value = "";
  publicKnowledgeCards.value = await api.listPublicKnowledgeCards().catch(() => []);
  publicPickOpen.value = true;
}

function closePublicPick() {
  if (publicPickBusy.value) return;
  publicPickOpen.value = false;
  publicPickWriteSide.value = null;
}

async function addPublicCardToWritePrompt(picked: PublicKnowledgeCard, side: WritePromptKind) {
  if (!tree.value) return;
  const hub = writePromptsHub(tree.value);
  if (!hub) return;
  const handles = writePromptEdgeHandles(side);
  const id = crypto.randomUUID();
  const knowledge = emptyKnowledge();
  knowledge.book_ids = [...(picked.book_ids ?? [])];
  knowledge.extract_prompt = picked.extract_prompt ?? "";
  knowledge.extracted = picked.extracted ?? "";
  const node: TreeNode = {
    id,
    kind: "knowledge",
    label: picked.title.trim() || t("workspace.newKnowledge"),
    outline: "",
    detailed_outline: [],
    character: null,
    knowledge,
    side_plot: null,
    linked_character_ids: [],
    linked_side_plot_ids: [],
    linked_knowledge_ids: [],
    position: { x: hub.position.x, y: hub.position.y },
    word_count: 0,
    word_count_min: 0,
    word_count_max: 0,
    chapter_count: 0,
  };
  hub.linked_knowledge_ids = hub.linked_knowledge_ids ?? [];
  hub.linked_knowledge_ids.push(id);
  tree.value.nodes.push(node);
  tree.value.edges.push({
    id: `e-${hub.id}-${id}`,
    source: hub.id,
    target: id,
    kind: "knowledge",
    source_handle: handles.source,
    target_handle: handles.target,
  });
  pruneTreeRefs(tree.value);
  applyAutoLayout(tree.value.nodes, tree.value.edges);
  const hubId = hub.id;
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
  const kept = tree.value.nodes.find((n) => n.id === hubId);
  if (kept) selected.value = kept;
}

async function unlinkWritePromptCard(cardId: string) {
  if (!tree.value || publicPickBusy.value) return;
  const hub = writePromptsHub(tree.value);
  if (!hub) return;
  const drop = tree.value.edges.filter(
    (e) =>
      e.kind === "knowledge" &&
      ((e.source === hub.id && e.target === cardId) ||
        (e.source === cardId && e.target === hub.id)),
  );
  if (!drop.length) return;
  publicPickBusy.value = true;
  try {
    for (const e of drop) unlinkEdgeRefs(e);
    const dropIds = new Set(drop.map((e) => e.id));
    tree.value.edges = tree.value.edges.filter((e) => !dropIds.has(e.id));
    const leftover = tree.value.edges.some((e) => e.source === cardId || e.target === cardId);
    if (!leftover) {
      tree.value.nodes = tree.value.nodes.filter((n) => n.id !== cardId);
    }
    pruneTreeRefs(tree.value);
    applyAutoLayout(tree.value.nodes, tree.value.edges);
    const hubId = hub.id;
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
    const kept = tree.value.nodes.find((n) => n.id === hubId);
    if (kept) selected.value = kept;
  } catch (e) {
    notice.value = String(e);
  } finally {
    publicPickBusy.value = false;
  }
}

async function addPickedPublicCard(id: string) {
  if (publicPickBusy.value) return;
  const picked = publicKnowledgeCards.value.find((c) => c.id === id);
  if (picked && publicCardAlreadyOnTree(picked)) return;
  publicPickBusy.value = true;
  try {
    const side = publicPickWriteSide.value;
    if (side && picked) {
      await addPublicCardToWritePrompt(picked, side);
    } else {
      const before = new Set((tree.value?.nodes ?? []).map((n) => n.id));
      tree.value = await api.addPublicKnowledgeCard(
        props.id,
        id,
        workspaceTab.value === "book"
          ? (tree.value?.nodes.find((n) => n.kind === "novel")?.id ?? null)
          : (selected.value?.id ?? null),
      );
      pruneTreeRefs(tree.value);
      syncFlowFromTree();
      const added = tree.value.nodes.find((n) => n.kind === "knowledge" && !before.has(n.id));
      if (added) await selectNode(added);
    }
    publicPickOpen.value = false;
    publicPickWriteSide.value = null;
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
    selected.value.character = syncLegacyFields(normalizeCharacterCard(payload.card));
    const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
    if (n) {
      n.label = payload.name;
      n.character = syncLegacyFields(normalizeCharacterCard(payload.card));
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
  if (sel.kind === "chapter" || sel.kind === "side_plot") {
    n.outline = sel.outline;
  }
  if (sel.kind === "chapter") {
    n.detailed_outline = [...(sel.detailed_outline ?? [])]
      .map((s) => String(s ?? "").trim())
      .filter(Boolean);
    sel.detailed_outline = [...n.detailed_outline];
  }
  if (sel.kind === "side_plot") {
    n.side_plot = sel.side_plot
      ? { ...sel.side_plot }
      : { status: "active", absorbed: false };
  }
  tree.value = await persistTree(tree.value);
  syncFlowFromTree();
}

/** 单条 AI 重写中的下标；null 表示未在重写 */
const detailedOutlineItemBusy = ref<number | null>(null);
/** 细纲单条 AI：先填提示再重写 */
const pendingDetailedOutlineAi = ref<{ index: number; text: string } | null>(null);
const detailedOutlineAiNote = ref("");
const detailedOutlineAiError = ref("");

function selectedChapterNumber(): number | null {
  if (!tree.value || !selected.value || selected.value.kind !== "chapter") return null;
  const chapters = sortNodesByCanvasPosition(tree.value.nodes.filter((n) => n.kind === "chapter"));
  const i = chapters.findIndex((c) => c.id === selected.value!.id);
  return i >= 0 ? i + 1 : null;
}

async function sendChapterChat(
  kind: "outline" | "generate" | "refine",
) {
  const n = selectedChapterNumber();
  if (n == null) return;
  if (
    (kind === "generate" || kind === "refine") &&
    selected.value?.kind === "chapter"
  ) {
    scheduleBodyAutosave.cancel();
    await persistChapterBody();
    bodyDiffOrigin.value = {
      nodeId: selected.value.id,
      before: stripChapterMeta(bodyDraft.value),
    };
  }
  const text =
    kind === "outline"
      ? chatCmdGenerateOutline(n)
      : kind === "generate"
        ? chatCmdGenerateChapter(n)
        : chatCmdRefineChapter(n);
  enqueueGlobalChatSend(text);
}

function ensureSelectedDetailedOutline(): string[] {
  if (!selected.value || selected.value.kind !== "chapter") return [];
  if (!Array.isArray(selected.value.detailed_outline)) selected.value.detailed_outline = [];
  return selected.value.detailed_outline;
}

function addDetailedOutlineItem() {
  if (!selected.value || selected.value.kind !== "chapter" || chapterBusy.value) return;
  ensureSelectedDetailedOutline().push("");
}

async function removeDetailedOutlineItem(i: number) {
  if (!selected.value || selected.value.kind !== "chapter" || chapterBusy.value) return;
  if (detailedOutlineItemBusy.value !== null) return;
  const list = ensureSelectedDetailedOutline();
  list.splice(i, 1);
  await persistSelectedCardText();
}

async function moveDetailedOutlineItem(i: number, dir: -1 | 1) {
  if (!selected.value || selected.value.kind !== "chapter" || chapterBusy.value) return;
  if (detailedOutlineItemBusy.value !== null) return;
  const list = ensureSelectedDetailedOutline();
  const j = i + dir;
  if (j < 0 || j >= list.length) return;
  const tmp = list[i];
  list[i] = list[j];
  list[j] = tmp;
  await persistSelectedCardText();
}

function openDetailedOutlineAiItem(i: number) {
  if (!selected.value || selected.value.kind !== "chapter" || chapterBusy.value) return;
  if (detailedOutlineItemBusy.value !== null) return;
  if (!(selected.value.outline ?? "").trim()) {
    notice.value = t("workspace.chapterDetailedOutlineNeedBrief");
    return;
  }
  const list = ensureSelectedDetailedOutline();
  if (i < 0 || i >= list.length) return;
  pendingDetailedOutlineAi.value = {
    index: i,
    text: list[i] ?? "",
  };
  detailedOutlineAiNote.value = "";
  detailedOutlineAiError.value = "";
}

function cancelDetailedOutlineAi() {
  if (detailedOutlineItemBusy.value !== null) return;
  pendingDetailedOutlineAi.value = null;
  detailedOutlineAiNote.value = "";
  detailedOutlineAiError.value = "";
}

async function confirmDetailedOutlineAi() {
  const pending = pendingDetailedOutlineAi.value;
  if (!pending || !tree.value || !selected.value || selected.value.kind !== "chapter") return;
  if (chapterBusy.value || detailedOutlineItemBusy.value !== null) return;
  const notes = detailedOutlineAiNote.value.trim();
  notice.value = "";
  detailedOutlineAiError.value = "";
  try {
    await persistSelectedCardText();
    detailedOutlineItemBusy.value = pending.index;
    const list = ensureSelectedDetailedOutline();
    const item = await api.regenerateDetailedOutlineItem(
      props.id,
      selected.value.id,
      pending.index,
      notes,
    );
    list[pending.index] = item;
    const n = tree.value.nodes.find((x) => x.id === selected.value!.id);
    if (n) n.detailed_outline = [...list];
    selected.value.detailed_outline = [...list];
    tree.value = await persistTree(tree.value);
    syncFlowFromTree();
    notice.value = t("workspace.chapterDetailedOutlineItemDone");
    pendingDetailedOutlineAi.value = null;
    detailedOutlineAiNote.value = "";
  } catch (e) {
    detailedOutlineAiError.value = String(e);
  } finally {
    detailedOutlineItemBusy.value = null;
  }
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

async function persistNovelFeatures(next: NovelFeatures) {
  if (!novel.value) return;
  try {
    novel.value = await api.updateNovelFeatures(props.id, normalizeNovelFeatures(next));
  } catch (e) {
    notice.value = String(e);
  }
}

/** 卷/章人物：继承只读（根 / 父卷），本机可 + */
function charNodeLabel(n: TreeNode): string {
  return n.label?.trim() || t("workspace.newCharacter");
}

type HostCharChip = {
  id: string;
  label: string;
  from: "root" | "volume" | "local";
};

function mapCharIds(
  ids: string[],
  tr: { nodes: TreeNode[] },
  from: HostCharChip["from"],
): HostCharChip[] {
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  const out: HostCharChip[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (!n || n.kind !== "character") continue;
    out.push({ id, label: charNodeLabel(n), from });
  }
  return out;
}

const hostCharPickerOpen = ref(false);

const hostCharChips = computed((): HostCharChip[] => {
  const sel = selected.value;
  const tr = tree.value;
  if (!sel || !tr || (sel.kind !== "chapter" && sel.kind !== "volume")) return [];
  if (sel.kind === "volume") {
    return [
      ...mapCharIds(rootLinkedCharacterIds(tr.nodes, tr.edges), tr, "root"),
      ...mapCharIds(volumeLocalCharacterIds(sel.id, tr.nodes, tr.edges), tr, "local"),
    ];
  }
  const rootIds = rootLinkedCharacterIds(tr.nodes, tr.edges);
  const rootSet = new Set(rootIds);
  const volId = chapterParentVolumeId(sel.id, tr.nodes, tr.edges);
  const volIds = volId
    ? volumeLocalCharacterIds(volId, tr.nodes, tr.edges).filter((id) => !rootSet.has(id))
    : [];
  return [
    ...mapCharIds(rootIds, tr, "root"),
    ...mapCharIds(volIds, tr, "volume"),
    ...mapCharIds(chapterLocalCharacterIds(sel.id, tr.nodes, tr.edges), tr, "local"),
  ];
});

const hostUnlinkedChars = computed(() => {
  const sel = selected.value;
  const tr = tree.value;
  if (!sel || !tr || (sel.kind !== "chapter" && sel.kind !== "volume")) return [];
  const have = new Set(hostCharChips.value.map((c) => c.id));
  return tr.nodes
    .filter((n) => n.kind === "character" && !have.has(n.id))
    .map((n) => ({ id: n.id, label: charNodeLabel(n) }))
    .sort((a, b) => a.label.localeCompare(b.label) || a.id.localeCompare(b.id));
});

async function linkCharacterToSelected(charId: string) {
  const sel = selected.value;
  const tr = tree.value;
  if (!tr || !sel || (sel.kind !== "chapter" && sel.kind !== "volume")) return;
  const inherited =
    sel.kind === "volume"
      ? new Set(rootLinkedCharacterIds(tr.nodes, tr.edges))
      : new Set(chapterInheritedCharacterIds(sel.id, tr.nodes, tr.edges));
  if (inherited.has(charId)) return;
  const host = tr.nodes.find((n) => n.id === sel.id);
  if (!host) return;
  host.linked_character_ids = host.linked_character_ids ?? [];
  if (!host.linked_character_ids.includes(charId)) host.linked_character_ids.push(charId);
  const exists = tr.edges.some(
    (e) =>
      (e.source === host.id && e.target === charId) ||
      (e.source === charId && e.target === host.id),
  );
  if (!exists) {
    tr.edges.push({
      id: `e-${host.id}-${charId}`,
      source: host.id,
      target: charId,
      kind: "character",
      source_handle: "left",
      target_handle: "right",
    });
  }
  tree.value = await persistTree(tr);
  syncFlowFromTree();
  const cur = tree.value.nodes.find((x) => x.id === host.id);
  if (cur) selected.value = cur;
  hostCharPickerOpen.value = false;
}

async function unlinkCharacterFromSelected(charId: string) {
  const sel = selected.value;
  const tr = tree.value;
  if (!tr || !sel || (sel.kind !== "chapter" && sel.kind !== "volume")) return;
  const inherited =
    sel.kind === "volume"
      ? new Set(rootLinkedCharacterIds(tr.nodes, tr.edges))
      : new Set(chapterInheritedCharacterIds(sel.id, tr.nodes, tr.edges));
  if (inherited.has(charId)) return;
  const host = tr.nodes.find((n) => n.id === sel.id);
  if (!host) return;
  host.linked_character_ids = (host.linked_character_ids ?? []).filter((id) => id !== charId);
  tr.edges = tr.edges.filter(
    (e) =>
      !((e.source === host.id && e.target === charId) || (e.source === charId && e.target === host.id)),
  );
  tree.value = await persistTree(tr);
  syncFlowFromTree();
  const cur = tree.value.nodes.find((x) => x.id === host.id);
  if (cur) selected.value = cur;
}

function plotNodeLabel(n: TreeNode): string {
  return n.label?.trim() || t("workspace.newPlot");
}

function mapPlotIds(
  ids: string[],
  tr: { nodes: TreeNode[] },
  from: HostCharChip["from"],
): HostCharChip[] {
  const byId = new Map(tr.nodes.map((n) => [n.id, n]));
  const out: HostCharChip[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (!n || n.kind !== "side_plot") continue;
    out.push({ id, label: plotNodeLabel(n), from });
  }
  return out;
}

const hostPlotPickerOpen = ref(false);

const chapterPlotChips = computed((): HostCharChip[] => {
  const sel = selected.value;
  const tr = tree.value;
  if (!sel || !tr || sel.kind !== "chapter") return [];
  const rootIds = rootLinkedPlotIds(tr.nodes, tr.edges);
  const rootSet = new Set(rootIds);
  const volId = chapterParentVolumeId(sel.id, tr.nodes, tr.edges);
  const volIds = volId
    ? volumeLocalPlotIds(volId, tr.nodes, tr.edges).filter((id) => !rootSet.has(id))
    : [];
  return [
    ...mapPlotIds(rootIds, tr, "root"),
    ...mapPlotIds(volIds, tr, "volume"),
    ...mapPlotIds(chapterLocalPlotIds(sel.id, tr.nodes, tr.edges), tr, "local"),
  ];
});

const chapterUnlinkedPlots = computed(() => {
  const sel = selected.value;
  const tr = tree.value;
  if (!sel || !tr || sel.kind !== "chapter") return [];
  const have = new Set(chapterPlotChips.value.map((c) => c.id));
  return tr.nodes
    .filter((n) => n.kind === "side_plot" && !have.has(n.id))
    .map((n) => ({ id: n.id, label: plotNodeLabel(n) }))
    .sort((a, b) => a.label.localeCompare(b.label) || a.id.localeCompare(b.id));
});

async function linkPlotToSelected(plotId: string) {
  const sel = selected.value;
  const tr = tree.value;
  if (!tr || !sel || sel.kind !== "chapter") return;
  if (chapterInheritedPlotIds(sel.id, tr.nodes, tr.edges).includes(plotId)) return;
  const host = tr.nodes.find((n) => n.id === sel.id);
  if (!host) return;
  host.linked_side_plot_ids = host.linked_side_plot_ids ?? [];
  if (!host.linked_side_plot_ids.includes(plotId)) host.linked_side_plot_ids.push(plotId);
  const exists = tr.edges.some(
    (e) =>
      (e.source === host.id && e.target === plotId) ||
      (e.source === plotId && e.target === host.id),
  );
  if (!exists) {
    tr.edges.push({
      id: `e-${host.id}-${plotId}`,
      source: host.id,
      target: plotId,
      kind: "side_plot",
      source_handle: "right",
      target_handle: "left",
    });
  }
  tree.value = await persistTree(tr);
  syncFlowFromTree();
  const cur = tree.value.nodes.find((x) => x.id === host.id);
  if (cur) selected.value = cur;
  hostPlotPickerOpen.value = false;
}

async function unlinkPlotFromSelected(plotId: string) {
  const sel = selected.value;
  const tr = tree.value;
  if (!tr || !sel || sel.kind !== "chapter") return;
  if (chapterInheritedPlotIds(sel.id, tr.nodes, tr.edges).includes(plotId)) return;
  const host = tr.nodes.find((n) => n.id === sel.id);
  if (!host) return;
  host.linked_side_plot_ids = (host.linked_side_plot_ids ?? []).filter((id) => id !== plotId);
  tr.edges = tr.edges.filter(
    (e) =>
      !((e.source === host.id && e.target === plotId) || (e.source === plotId && e.target === host.id)),
  );
  tree.value = await persistTree(tr);
  syncFlowFromTree();
  const cur = tree.value.nodes.find((x) => x.id === host.id);
  if (cur) selected.value = cur;
}

watch(
  () => selected.value?.id,
  () => {
    hostCharPickerOpen.value = false;
    hostPlotPickerOpen.value = false;
  },
);

const chapterBusy = computed(
  () =>
    busy.value === "plan-next" ||
    busy.value === "gen-plots" ||
    busy.value === "regen-memory" ||
    busy.value === "gen-cards" ||
    busy.value === "consolidate-plots" ||
    busy.value === "split-shots" ||
    busy.value === "shot-prompts" ||
    busy.value === "submit-comfy",
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
const MIN_WV_NAV = 120;
const MAX_WV_NAV = 220;
const wvNavW = useLocalStorage("novework.workspaceWvNavW", 152);
const showBodyDock = computed(
  () => workspaceTab.value === "manuscript" && selected.value?.kind === "chapter",
);

const editorOnRight = computed(() => {
  const tab = workspaceTab.value;
  return (
    tab === "book" ||
    tab === "worldview" ||
    tab === "characters" ||
    tab === "story-rules" ||
    tab === "plots"
  );
});

const workspaceGridStyle = computed(() => {
  if (workspaceTab.value === "manuscript") {
    const listCol = `minmax(0, ${leftW.value}px) 6px minmax(0, 1fr)`;
    if (!showBodyDock.value) {
      return {
        display: "grid",
        height: "100%",
        gridTemplateColumns: listCol,
        gridTemplateRows: "minmax(0, 1fr)",
        gridTemplateAreas: `"mid v1 left"`,
      };
    }
    if (chatDock.value === "bottom") {
      return {
        display: "grid",
        height: "100%",
        gridTemplateColumns: listCol,
        gridTemplateRows: `minmax(0, 1fr) 6px ${chatBottomH.value}px`,
        gridTemplateAreas: `"mid v1 left" "mid v1 hr" "mid v1 chat"`,
      };
    }
    return {
      display: "grid",
      height: "100%",
      gridTemplateColumns: `${listCol} 6px minmax(0, ${rightW.value}px)`,
      gridTemplateRows: "minmax(0, 1fr)",
      gridTemplateAreas: `"mid v1 left v2 chat"`,
    };
  }
  if (editorOnRight.value) {
    return {
      display: "grid",
      height: "100%",
      gridTemplateColumns: `${wvNavW.value}px 6px minmax(0, 1fr)`,
      gridTemplateRows: "minmax(0, 1fr)",
      gridTemplateAreas: `"mid v1 left"`,
    };
  }
  return {
    display: "grid",
    height: "100%",
    gridTemplateColumns: `${leftW.value}px 6px minmax(0, 1fr)`,
    gridTemplateRows: "minmax(0, 1fr)",
    gridTemplateAreas: `"left v1 mid"`,
  };
});

function startResize(which: "left" | "right", ev: MouseEvent) {
  ev.preventDefault();
  const startX = ev.clientX;
  const startLeft = leftW.value;
  const startRight = rightW.value;
  const startWv = wvNavW.value;
  const onMove = (e: MouseEvent) => {
    const dx = e.clientX - startX;
    const total = window.innerWidth;
    if (which === "left") {
      const max =
        !showBodyDock.value || chatDock.value === "bottom"
          ? total - MIN_MID - 16
          : total - rightW.value - MIN_MID - 16;
      if (editorOnRight.value) {
        wvNavW.value = Math.min(Math.max(startWv + dx, MIN_WV_NAV), Math.min(MAX_WV_NAV, max));
        return;
      }
      const next = startLeft + dx;
      leftW.value = Math.min(Math.max(next, MIN_SIDE), max);
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

</script>

<template>
  <div class="flex h-full min-w-0 flex-col overflow-hidden" tabindex="0" @keydown="onCanvasKeydown">
    <WorkspaceTabBar v-model="workspaceTab" />
    <div class="min-h-0 min-w-0 w-full flex-1 overflow-hidden" :style="workspaceGridStyle">
      <!-- 左：卡片属性 -->
      <div
        class="flex min-h-0 min-w-0 flex-col overflow-hidden"
        :class="editorOnRight || workspaceTab === 'manuscript' ? 'border-l' : 'border-r'"
        style="grid-area: left"
      >
        <template v-if="workspaceTab !== 'plots' || selected?.kind === 'side_plot'">
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
            <div v-else class="min-w-0 flex-1 truncate text-sm font-medium">
              {{ selected?.label?.trim() || t("workspace.selectNode") }}
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
            selectedIsChapter || selectedIsNovel || selectedIsVolume
              ? 'overflow-y-auto'
              : selected?.kind === 'side_plot' || selectedIsPlainKnowledge
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
                busy === "plan-next"
                  ? t("workspace.planningNext")
                  : busy === "gen-plots"
                    ? t("workspace.genPlotsBusy")
                    : busy === "regen-memory"
                      ? t("workspace.regenMemoryBusy")
                      : busy === "gen-cards"
                        ? t("workspace.rootGenChaptersBusy")
                        : busy === "consolidate-plots"
                          ? t("workspace.consolidatePlotsBusy")
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
            :novel-id="id"
            :character-id="selected?.id"
            :name="selected?.label ?? ''"
            :card="selected?.character ?? emptyCharacter()"
            :busy="characterSaveBusy"
            :law-options="characterLawOptions"
            :character-names="otherCharacterNames"
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
          <HistoryCulturePanel
            v-else-if="selectedIsHistoryCulture"
            :novel-id="id"
            :data="selected?.knowledge?.history_culture"
            :religion-links="historyCultureReligionLinks"
            :event-links="historyCultureEventLinks"
            :linked-religions="historyCultureLinkedReligions"
            :linked-events="historyCultureLinkedEvents"
            @save="persistSelectedHistoryCulture"
            @add-religion="addHistoryCultureReligion"
            @add-event="addHistoryCultureEvent"
            @open-religion="openHistoryCultureReligion"
            @open-event="openHistoryCultureEvent"
          />
          <WorldReligionPanel
            v-else-if="selectedIsWorldReligion"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.world_religion"
            @save="persistSelectedWorldReligion"
          />
          <MajorEventPanel
            v-else-if="selectedIsMajorEvent"
            :novel-id="id"
            :name="selected?.label ?? ''"
            :data="selected?.knowledge?.major_event"
            @save="persistSelectedMajorEvent"
          />
          <StoryRulesHubPanel
            v-else-if="selectedIsStoryRules"
            :block-links="storyRulesBlockLinks"
            @open-block="openStoryRulesBlock"
            @open-chat="openStoryRulesChat"
          />
          <StoryRulesBlockPanel
            v-else-if="selectedIsStoryRulesBlock && selectedStoryRulesBlockSlot"
            :novel-id="id"
            :node-id="selected?.id"
            :slot="selectedStoryRulesBlockSlot"
            :data="selected?.knowledge"
            @save="persistSelectedStoryRulesBlock"
          />
          <div v-else-if="selectedIsWritePrompts" class="space-y-3 text-sm">
            <h3 class="text-sm font-semibold">{{ t("workspace.writePrompts.title") }}</h3>
            <p class="text-[11px] leading-relaxed text-muted-foreground">{{ t("workspace.writePrompts.hint") }}</p>
            <div
              v-for="panel in writePromptPanels"
              :key="panel.side"
              class="space-y-1.5 rounded-md border p-2.5"
            >
              <label class="block text-sm font-semibold">{{ t(panel.titleKey) }}</label>
              <div class="flex flex-wrap items-center gap-1.5">
                <span
                  v-for="c in panel.cards"
                  :key="c.id"
                  class="inline-flex max-w-full items-center rounded px-2 py-1 text-xs"
                  :class="panel.chipClass"
                >
                  <button
                    type="button"
                    class="min-w-0 truncate"
                    @click="openTreeNode(c.id)"
                  >{{ knowledgeNodeLabel(c, t) }}</button>
                  <button
                    type="button"
                    class="ml-0.5 inline-flex h-3 w-3 shrink-0 items-center justify-center rounded-sm opacity-70 hover:bg-black/10 hover:opacity-100"
                    :disabled="publicPickBusy"
                    :title="t('workspace.hostUnlinkCharacter')"
                    @click="unlinkWritePromptCard(c.id)"
                  >
                    <X class="h-2.5 w-2.5" />
                  </button>
                </span>
                <p
                  v-if="!panel.cards.length"
                  class="text-[11px] text-muted-foreground"
                >{{ t("workspace.writePrompts.empty") }}</p>
                <button
                  type="button"
                  class="flex h-5 w-5 items-center justify-center rounded border border-dashed"
                  :class="panel.plusClass"
                  :disabled="publicPickBusy"
                  :title="t('workspace.addPublicKnowledge')"
                  @click="openPublicPick(panel.side)"
                >
                  <Plus class="h-3 w-3" />
                </button>
              </div>
            </div>
          </div>
          <div v-else-if="selectedIsKnowledge" class="flex min-h-0 flex-1 flex-col space-y-4 text-sm">
            <div class="shrink-0">
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
            <p class="shrink-0 text-[11px] text-muted-foreground">{{ t("workspace.knowledgeHint") }}</p>
            <div class="flex min-h-0 flex-1 flex-col space-y-1">
              <label class="block shrink-0 text-xs text-muted-foreground">{{ t("workspace.knowledgeFeatures") }}</label>
              <div class="relative min-h-0 flex-1">
                <Textarea
                  :model-value="knowledgeDraft.extracted"
                  class="absolute inset-0 resize-none overflow-y-auto text-sm"
                  :placeholder="t('workspace.knowledgeFeaturesPh')"
                  @update:model-value="(v) => { const k = ensureKnowledgePayload(); if (k) k.extracted = String(v); }"
                  @change="persistSelectedKnowledge"
                />
              </div>
              <p class="shrink-0 text-[11px] text-muted-foreground">
                {{
                  t("workspace.knowledgeFeaturesHint", {
                    n: (knowledgeDraft.extracted || "").length,
                    cap: 500,
                  })
                }}
              </p>
            </div>
          </div>
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
              <label class="block text-xs font-medium text-muted-foreground">{{ t("novels.feat.title") }}</label>
              <p class="text-[11px] text-muted-foreground">{{ t("novels.feat.hint") }}</p>
              <NovelFeaturesPicker
                :model-value="novel?.features ?? emptyNovelFeatures()"
                @update:model-value="persistNovelFeatures"
              />
            </div>
            <div class="space-y-2">
              <label class="block text-xs text-muted-foreground">{{ t("workspace.rootOutline") }}</label>
              <Textarea
                :model-value="selected?.outline ?? ''"
                rows="4"
                class="field-sizing-content resize-y text-sm !min-h-24"
                :placeholder="t('workspace.rootOutlinePh')"
                @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                @change="persistSelectedCardText"
              />
              <p class="text-xs text-muted-foreground">{{ t("workspace.rootOutlineHint") }}</p>
            </div>
          </div>
          <div v-else-if="selectedIsVolume" class="space-y-3">
              <div class="space-y-1.5">
              <label class="block text-sm font-semibold">{{ t("workspace.hostLinkedCharacters") }}</label>
              <div class="flex flex-wrap items-center gap-1.5">
                <span
                  v-for="c in hostCharChips"
                  :key="c.from + c.id"
                  class="inline-flex items-center rounded px-2 py-1 text-xs"
                  :class="
                    c.from === 'local'
                      ? 'bg-amber-100 text-amber-900'
                      : c.from === 'volume'
                        ? 'border border-dashed border-violet-300 bg-violet-50/70 text-violet-900 dark:border-violet-800 dark:bg-violet-950/30 dark:text-violet-100'
                        : 'border border-dashed bg-muted/40 text-muted-foreground'
                  "
                  :title="
                    c.from === 'root'
                      ? t('workspace.charInheritedHintRoot')
                      : c.from === 'volume'
                        ? t('workspace.charInheritedHintVolume')
                        : t('workspace.role')
                  "
                >
                  <span
                    v-if="c.from !== 'local'"
                    class="mr-0.5 font-medium"
                  >{{
                    c.from === "root"
                      ? t("workspace.chapterPlotInherited")
                      : t("workspace.volumePlotInherited")
                  }}</span>{{ c.label }}
                  <button
                    v-if="c.from === 'local'"
                    type="button"
                    class="ml-0.5 inline-flex h-3 w-3 items-center justify-center rounded-sm text-amber-800/80 hover:bg-amber-200 hover:text-amber-950"
                    :title="t('workspace.hostUnlinkCharacter')"
                    @click="unlinkCharacterFromSelected(c.id)"
                  >
                    <X class="h-2.5 w-2.5" />
                  </button>
                </span>
                <div class="relative">
                  <button
                    type="button"
                    class="flex h-5 w-5 items-center justify-center rounded border border-dashed border-amber-400 text-amber-800 hover:bg-amber-50 dark:hover:bg-amber-950/40"
                    :title="t('workspace.hostAddCharacter')"
                    @click="hostCharPickerOpen = !hostCharPickerOpen"
                  >
                    <Plus class="h-3 w-3" />
                  </button>
                  <div
                    v-if="hostCharPickerOpen"
                    class="absolute left-0 top-full z-20 mt-1 max-h-48 min-w-[10rem] overflow-y-auto rounded-md border bg-background py-1 shadow-md"
                  >
                    <p
                      v-if="!hostUnlinkedChars.length"
                      class="px-2 py-1.5 text-[11px] text-muted-foreground"
                    >{{ t("workspace.hostUnlinkedCharsEmpty") }}</p>
                    <button
                      v-for="c in hostUnlinkedChars"
                      :key="c.id"
                      type="button"
                      class="block w-full truncate px-2 py-1 text-left text-xs hover:bg-amber-50 dark:hover:bg-amber-950/40"
                      @click="linkCharacterToSelected(c.id)"
                    >{{ c.label }}</button>
                  </div>
                </div>
              </div>
              </div>
              <VolumePanel
                :novel-id="id"
                :node-id="selected?.id"
                :data="selected?.volume"
                @save="persistSelectedVolume"
              />
          </div>
          <div v-else-if="selectedIsChapter" class="space-y-3">
            <div class="space-y-1.5">
              <label class="block text-sm font-semibold">{{ t("workspace.hostLinkedCharacters") }}</label>
              <div class="flex flex-wrap items-center gap-1.5">
              <span
                v-for="c in hostCharChips"
                :key="c.from + c.id"
                class="inline-flex items-center rounded px-2 py-1 text-xs"
                :class="
                  c.from === 'local'
                    ? 'bg-amber-100 text-amber-900'
                    : c.from === 'volume'
                      ? 'border border-dashed border-violet-300 bg-violet-50/70 text-violet-900 dark:border-violet-800 dark:bg-violet-950/30 dark:text-violet-100'
                      : 'border border-dashed bg-muted/40 text-muted-foreground'
                "
                :title="
                  c.from === 'root'
                    ? t('workspace.charInheritedHintRoot')
                    : c.from === 'volume'
                      ? t('workspace.charInheritedHintVolume')
                      : t('workspace.role')
                "
              >
                <span
                  v-if="c.from !== 'local'"
                  class="mr-0.5 font-medium"
                >{{
                  c.from === "root"
                    ? t("workspace.chapterPlotInherited")
                    : t("workspace.volumePlotInherited")
                }}</span>{{ c.label }}
                <button
                  v-if="c.from === 'local'"
                  type="button"
                  class="ml-0.5 inline-flex h-3 w-3 items-center justify-center rounded-sm text-amber-800/80 hover:bg-amber-200 hover:text-amber-950"
                  :title="t('workspace.hostUnlinkCharacter')"
                  @click="unlinkCharacterFromSelected(c.id)"
                >
                  <X class="h-2.5 w-2.5" />
                </button>
              </span>
              <div class="relative">
                <button
                  type="button"
                  class="flex h-5 w-5 items-center justify-center rounded border border-dashed border-amber-400 text-amber-800 hover:bg-amber-50 dark:hover:bg-amber-950/40"
                  :title="t('workspace.hostAddCharacter')"
                  @click="hostCharPickerOpen = !hostCharPickerOpen"
                >
                  <Plus class="h-3 w-3" />
                </button>
                <div
                  v-if="hostCharPickerOpen"
                  class="absolute left-0 top-full z-20 mt-1 max-h-48 min-w-[10rem] overflow-y-auto rounded-md border bg-background py-1 shadow-md"
                >
                  <p
                    v-if="!hostUnlinkedChars.length"
                    class="px-2 py-1.5 text-[11px] text-muted-foreground"
                  >{{ t("workspace.hostUnlinkedCharsEmpty") }}</p>
                  <button
                    v-for="c in hostUnlinkedChars"
                    :key="c.id"
                    type="button"
                    class="block w-full truncate px-2 py-1 text-left text-xs hover:bg-amber-50 dark:hover:bg-amber-950/40"
                    @click="linkCharacterToSelected(c.id)"
                  >{{ c.label }}</button>
                </div>
              </div>
              </div>
            </div>
            <div class="space-y-1.5">
              <label class="block text-sm font-semibold">{{ t("workspace.chapterBriefOutline") }}</label>
              <Textarea
                :model-value="selected?.outline ?? ''"
                rows="2"
                class="field-sizing-content resize-y text-sm !min-h-12"
                :placeholder="t('workspace.chapterOutlinePh')"
                :disabled="chapterBusy"
                @update:model-value="(v) => { if (selected) selected.outline = String(v); }"
                @change="persistSelectedCardText"
              />
              <p class="text-xs text-muted-foreground">{{ t("workspace.chapterOutlineHint") }}</p>
            </div>

            <div class="space-y-1.5">
              <label class="block text-sm font-semibold">{{ t("workspace.chapterLinkedPlots") }}</label>
              <div class="flex flex-wrap items-center gap-1.5">
                <span
                  v-for="p in chapterPlotChips"
                  :key="p.from + p.id"
                  class="inline-flex items-center rounded px-2 py-1 text-xs"
                  :class="
                    p.from === 'local'
                      ? 'bg-sky-100 text-sky-900'
                      : p.from === 'volume'
                        ? 'border border-dashed border-violet-300 bg-violet-50/70 text-violet-900 dark:border-violet-800 dark:bg-violet-950/30 dark:text-violet-100'
                        : 'border border-dashed bg-muted/40 text-muted-foreground'
                  "
                  :title="
                    p.from === 'root'
                      ? t('workspace.charInheritedHintRoot')
                      : p.from === 'volume'
                        ? t('workspace.charInheritedHintVolume')
                        : t('workspace.plot')
                  "
                >
                  <span
                    v-if="p.from !== 'local'"
                    class="mr-0.5 font-medium"
                  >{{
                    p.from === "root"
                      ? t("workspace.chapterPlotInherited")
                      : t("workspace.volumePlotInherited")
                  }}</span>{{ p.label }}
                  <button
                    v-if="p.from === 'local'"
                    type="button"
                    class="ml-0.5 inline-flex h-3 w-3 items-center justify-center rounded-sm text-sky-800/80 hover:bg-sky-200 hover:text-sky-950"
                    :title="t('workspace.hostUnlinkPlot')"
                    @click="unlinkPlotFromSelected(p.id)"
                  >
                    <X class="h-2.5 w-2.5" />
                  </button>
                </span>
                <div class="relative">
                  <button
                    type="button"
                    class="flex h-5 w-5 items-center justify-center rounded border border-dashed border-sky-400 text-sky-800 hover:bg-sky-50 dark:hover:bg-sky-950/40"
                    :title="t('workspace.hostAddPlot')"
                    @click="hostPlotPickerOpen = !hostPlotPickerOpen"
                  >
                    <Plus class="h-3 w-3" />
                  </button>
                  <div
                    v-if="hostPlotPickerOpen"
                    class="absolute left-0 top-full z-20 mt-1 max-h-48 min-w-[10rem] overflow-y-auto rounded-md border bg-background py-1 shadow-md"
                  >
                    <p
                      v-if="!chapterUnlinkedPlots.length"
                      class="px-2 py-1.5 text-[11px] text-muted-foreground"
                    >{{ t("workspace.hostUnlinkedPlotsEmpty") }}</p>
                    <button
                      v-for="p in chapterUnlinkedPlots"
                      :key="p.id"
                      type="button"
                      class="block w-full truncate px-2 py-1 text-left text-xs hover:bg-sky-50 dark:hover:bg-sky-950/40"
                      @click="linkPlotToSelected(p.id)"
                    >{{ p.label }}</button>
                  </div>
                </div>
              </div>
            </div>

            <div class="space-y-2">
              <div class="flex items-center justify-between gap-2">
                <label class="text-sm font-semibold">{{ t("workspace.chapterDetailedOutline") }}</label>
                <div class="flex flex-wrap items-center justify-end gap-1">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    class="h-7 px-2 text-xs"
                    :disabled="chapterBusy || detailedOutlineItemBusy !== null"
                    :title="t('workspace.chapterDetailedOutlineGenHint')"
                    @click="sendChapterChat('outline')"
                  >
                    {{ t("workspace.chapterDetailedOutlineGen") }}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    class="h-7 px-2 text-xs"
                    :disabled="chapterBusy || detailedOutlineItemBusy !== null"
                    @click="addDetailedOutlineItem"
                  >
                    <Plus class="mr-1 h-3.5 w-3.5" />
                    {{ t("workspace.chapterDetailedOutlineAdd") }}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    class="h-7 px-2 text-xs"
                    :disabled="chapterBusy || detailedOutlineItemBusy !== null"
                    :title="t('workspace.genChapterChatHint')"
                    @click="sendChapterChat('generate')"
                  >
                    {{ t("workspace.genChapterChat") }}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    class="h-7 px-2 text-xs"
                    :disabled="chapterBusy || detailedOutlineItemBusy !== null || (bodyWordCount < 1 && !(selected?.word_count))"
                    :title="t('workspace.refineChatHint')"
                    @click="sendChapterChat('refine')"
                  >
                    {{ t("workspace.refine") }}
                  </Button>
                </div>
              </div>
              <ul
                v-if="(selected?.detailed_outline ?? []).length"
                class="space-y-1.5"
              >
                  <li
                    v-for="(_, i) in selected!.detailed_outline!"
                    :key="'do-' + selected!.id + '-' + i"
                    class="flex items-start gap-1 rounded-md border bg-sky-50/40 p-2 dark:bg-sky-950/20"
                  >
                    <div class="mt-0.5 flex w-5 shrink-0 flex-col items-center gap-0">
                      <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        class="h-5 w-5"
                        :title="t('workspace.chapterDetailedOutlineMoveUp')"
                        :disabled="
                          chapterBusy ||
                          detailedOutlineItemBusy !== null ||
                          i === 0
                        "
                        @click="moveDetailedOutlineItem(i, -1)"
                      >
                        <ChevronUp class="h-3 w-3" />
                      </Button>
                      <span class="text-[10px] leading-none text-muted-foreground">{{ i + 1 }}</span>
                      <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        class="h-5 w-5"
                        :title="t('workspace.chapterDetailedOutlineMoveDown')"
                        :disabled="
                          chapterBusy ||
                          detailedOutlineItemBusy !== null ||
                          i >= (selected!.detailed_outline!.length - 1)
                        "
                        @click="moveDetailedOutlineItem(i, 1)"
                      >
                        <ChevronDown class="h-3 w-3" />
                      </Button>
                    </div>
                    <Textarea
                      :model-value="selected!.detailed_outline![i]"
                      rows="1"
                      class="min-h-8 flex-1 field-sizing-content resize-y py-1.5 text-sm"
                      :placeholder="t('workspace.chapterDetailedOutlinePh')"
                      :disabled="chapterBusy || detailedOutlineItemBusy !== null"
                      @update:model-value="(v) => { if (selected?.detailed_outline) selected.detailed_outline[i] = String(v); }"
                      @change="persistSelectedCardText"
                    />
                    <div class="mt-0.5 flex shrink-0 flex-col gap-0.5">
                      <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        class="h-7 w-7"
                        :title="t('workspace.chapterDetailedOutlineItemAi')"
                        :disabled="
                          chapterBusy ||
                          detailedOutlineItemBusy !== null ||
                          !(selected?.outline ?? '').trim()
                        "
                        @click="openDetailedOutlineAiItem(i)"
                      >
                        <Loader2
                          v-if="detailedOutlineItemBusy === i"
                          class="h-3.5 w-3.5 animate-spin"
                        />
                        <Sparkles v-else class="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        type="button"
                        size="icon"
                        variant="ghost"
                        class="h-7 w-7"
                        :disabled="chapterBusy || detailedOutlineItemBusy !== null"
                        @click="removeDetailedOutlineItem(i)"
                      >
                        <Trash2 class="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </li>
                </ul>
                <p v-else class="text-[11px] text-muted-foreground">{{ t("workspace.chapterDetailedOutlineEmpty") }}</p>
                <p class="text-[11px] text-muted-foreground">{{ t("workspace.chapterDetailedOutlineHint") }}</p>
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
        </template>
      </div>

      <div
        class="cursor-col-resize bg-border hover:bg-primary/40"
        style="grid-area: v1"
        :title="t('workspace.resize')"
        @mousedown="startResize('left', $event)"
      />

      <!-- 中：当前 Tab 概览 -->
      <div class="relative flex min-h-0 min-w-0 flex-col overflow-hidden" style="grid-area: mid">
        <!-- 全书 / 世界观 / 人物 / 故事规则 / 自定义剧情：窄导航 -->
        <div
          v-if="editorOnRight"
          class="flex min-h-0 flex-1 flex-col overflow-hidden"
        >
          <div v-if="workspaceTab === 'book'" class="shrink-0 space-y-1 border-b p-1.5">
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :disabled="!tree"
              :title="t('workspace.addKnowledge')"
              @click="addCard('knowledge')"
            >
              <Library class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ t("workspace.addKnowledge") }}</span>
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :disabled="!tree"
              :title="t('workspace.addPublicKnowledge')"
              @click="openPublicPick"
            >
              <Share2 class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ t("workspace.addPublicKnowledge") }}</span>
            </Button>
          </div>
          <div v-else-if="workspaceTab === 'worldview'" class="shrink-0 border-b p-1.5">
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :disabled="worldviewBusy"
              :title="t('workspace.wv.hint')"
              @click="openWorldviewChat()"
            >
              <Loader2 v-if="worldviewBusy" class="mr-1 h-3.5 w-3.5 shrink-0 animate-spin" />
              <Sparkles v-else class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">
                {{
                  worldviewBusy
                    ? t("workspace.wv.busy")
                    : rootWorldviewComplete
                      ? t("workspace.wv.chatOpen")
                      : t("workspace.wv.generate")
                }}
              </span>
            </Button>
          </div>
          <div v-else-if="workspaceTab === 'story-rules'" class="shrink-0 border-b p-1.5">
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :title="t('workspace.sr.hubHint')"
              @click="openStoryRulesChat"
            >
              <Sparkles class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ t("workspace.sr.chat") }}</span>
            </Button>
          </div>
          <div v-else-if="workspaceTab === 'characters'" class="shrink-0 border-b p-1.5">
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :disabled="!tree"
              :title="t('workspace.addCharacter')"
              @click="addCard('character')"
            >
              <User class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ t("workspace.addCharacter") }}</span>
            </Button>
          </div>
          <div v-else class="shrink-0 border-b p-1.5">
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-full px-1.5 text-[11px]"
              :disabled="!tree"
              :title="t('workspace.addPlot')"
              @click="addCard('side_plot')"
            >
              <GitBranch class="mr-1 h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ t("workspace.addPlot") }}</span>
            </Button>
          </div>
          <nav
            class="min-h-0 flex-1 overflow-y-auto py-0.5"
            :aria-label="
              workspaceTab === 'book'
                ? t('workspace.tabBook')
                : workspaceTab === 'worldview'
                  ? t('workspace.tabWorldview')
                  : workspaceTab === 'characters'
                    ? t('workspace.tabCharacters')
                    : workspaceTab === 'story-rules'
                      ? t('workspace.tabStoryRules')
                      : t('workspace.tabPlots')
            "
          >
            <template v-for="(item, i) in sideNavItems" :key="item.id">
              <p
                v-if="item.role === 'group'"
                class="px-2 pt-2 pb-0.5 text-[10px] font-medium tracking-wide"
                :class="{
                  'text-rose-700 dark:text-rose-300': item.tone === 'rose',
                  'text-indigo-700 dark:text-indigo-300': item.tone === 'indigo',
                  'text-amber-800 dark:text-amber-200': item.tone === 'amber',
                  'text-sky-700 dark:text-sky-300': item.tone === 'sky',
                  'text-muted-foreground': !item.tone || item.tone === 'teal',
                }"
              >
                {{ item.label }}
              </p>
              <button
                v-else
                type="button"
                class="flex w-full text-left leading-snug hover:bg-muted/70"
                :class="[
                  item.role === 'parent'
                    ? 'px-2 py-1.5 text-xs font-medium'
                    : 'ml-2 border-l-2 px-2 py-1 text-[11px]',
                  item.role === 'parent' &&
                  i > 0 &&
                  sideNavItems[i - 1]?.role !== 'parent'
                    ? 'mt-4'
                    : '',
                  selected?.id === item.id ? 'bg-primary/10 text-primary' : '',
                  item.role === 'child' && item.tone === 'rose' ? 'border-rose-400' : '',
                  item.role === 'child' && item.tone === 'indigo' ? 'border-indigo-400' : '',
                  item.role === 'child' && item.tone === 'amber' ? 'border-amber-400' : '',
                  item.role === 'child' && item.tone === 'sky' ? 'border-sky-400' : '',
                  item.role === 'child' && (!item.tone || item.tone === 'teal') ? 'border-border' : '',
                ]"
                :title="item.label"
                @click="openTreeNode(item.id)"
              >
                <span
                  class="min-w-0 truncate"
                  :class="
                    selected?.id === item.id
                      ? ''
                      : item.role === 'parent'
                        ? 'text-foreground'
                        : 'text-muted-foreground'
                  "
                >
                  {{ item.label }}
                </span>
              </button>
            </template>
            <p
              v-if="workspaceTab === 'book' && !sideNavItems.some((x) => x.role === 'child')"
              class="px-2 py-2 text-[11px] text-muted-foreground"
            >{{ t("workspace.bookKnowledgeEmpty") }}</p>
            <p
              v-else-if="workspaceTab === 'characters' && !sideNavItems.length"
              class="px-2 py-2 text-[11px] text-muted-foreground"
            >{{ t("workspace.characterBoardEmpty") }}</p>
            <p
              v-else-if="workspaceTab === 'plots' && !sideNavItems.length"
              class="px-2 py-2 text-[11px] text-muted-foreground"
            >{{ t("workspace.plotsEmpty") }}</p>
          </nav>
        </div>

        <!-- 卷与章节 -->
        <div v-else-if="workspaceTab === 'manuscript'" class="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div class="flex shrink-0 flex-wrap items-center gap-1.5 border-b px-3 py-2">
            <Button size="sm" variant="outline" class="h-7 text-xs" :disabled="!tree" @click="addCard('volume')">
              <Layers class="mr-1 h-3.5 w-3.5" />
              {{ t("workspace.addVolume") }}
            </Button>
            <Button size="sm" variant="outline" class="h-7 text-xs" :disabled="!tree" @click="addCard('chapter')">
              <BookOpen class="mr-1 h-3.5 w-3.5" />
              {{ t("workspace.addChapter") }}
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-7 px-0"
              :disabled="!tree || wordStatsBusy"
              :title="t('workspace.wordStats')"
              :aria-label="t('workspace.wordStats')"
              @click="openWordStats"
            >
              <Loader2 v-if="wordStatsBusy" class="h-3.5 w-3.5 animate-spin" />
              <Hash v-else class="h-3.5 w-3.5" />
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="h-7 w-7 px-0"
              :disabled="!tree || allMemoryBusy"
              :title="t('workspace.allChapterMemory')"
              @click="openAllChapterMemory"
            >
              <Loader2 v-if="allMemoryBusy" class="h-3.5 w-3.5 animate-spin" />
              <Brain v-else class="h-3.5 w-3.5" />
            </Button>
          </div>
          <div class="min-h-0 flex-1 overflow-y-auto p-2">
            <p v-if="!manuscriptRows.length" class="px-2 py-6 text-center text-sm text-muted-foreground">
              {{ t("workspace.manuscriptEmpty") }}
            </p>
            <div v-else class="space-y-0.5">
              <div
                v-for="row in manuscriptRows"
                :key="row.kind + row.id"
                class="flex items-center gap-1 rounded-md px-1 py-0.5"
                :class="selected?.id === row.id ? 'bg-primary/10' : 'hover:bg-muted/50'"
                :style="{ paddingLeft: 4 + row.indent * 14 + 'px' }"
              >
                <button
                  type="button"
                  class="min-w-0 flex-1 truncate py-1 text-left text-xs"
                  :class="row.kind === 'volume' ? 'font-medium text-violet-800' : ''"
                  @click="openTreeNode(row.id)"
                >
                  {{ row.label }}
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- 结构树 -->
        <div
          v-else-if="workspaceTab === 'tree'"
          class="relative min-h-0 flex-1 overflow-hidden"
        >
          <div
            class="pointer-events-none absolute bottom-3 left-2 top-2 z-20 flex max-w-[calc(100%-1rem)] flex-col gap-1"
          >
            <div class="pointer-events-auto flex flex-wrap items-center gap-1.5">
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
                  :disabled="!tree"
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
                  :disabled="!tree"
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
                  :disabled="!tree"
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
                  :disabled="!tree"
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
                  :disabled="!tree"
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
                  :disabled="!tree"
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
                class="h-7 bg-background/95 px-2 text-xs"
                :disabled="!tree"
                :title="t('workspace.autoLayout')"
                @click="autoLayout"
              >
                <LayoutGrid class="mr-1 h-3.5 w-3.5" />
                {{ t("workspace.autoLayout") }}
              </Button>
            </div>
            <div
              class="w-fit max-w-full rounded bg-background/90 px-2 py-1 text-[10px] leading-snug text-muted-foreground shadow"
            >
              {{ t("workspace.flowHint") }}
            </div>
            <nav
              v-if="showChapterNav && tree"
              class="pointer-events-auto flex min-h-0 w-[9.5rem] flex-1 flex-col"
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
          <ReteCanvas
            ref="reteCanvas"
            class="h-full w-full"
            :nodes="flowNodes"
            :edges="flowEdges"
            :selected-id="selected?.id ?? null"
            @node-click="onNodeClick"
            @connect="onConnect"
            @edge-click="onEdgeClick"
            @edge-double-click="onEdgeDoubleClick"
            @node-drag-stop="onNodeDragStop"
          />
        </div>
      </div>

      <div
        v-if="showBodyDock && chatDock === 'right'"
        class="cursor-col-resize bg-border hover:bg-primary/40"
        style="grid-area: v2"
        :title="t('workspace.resize')"
        @mousedown="startResize('right', $event)"
      />

      <div
        v-if="showBodyDock && chatDock === 'bottom'"
        class="cursor-row-resize bg-border hover:bg-primary/40"
        style="grid-area: hr"
        :title="t('workspace.resizeChatH')"
        @mousedown="startResizeChatH($event)"
      />

      <!-- 章节正文：仅「卷与章节」Tab -->
      <div
        v-if="showBodyDock"
        class="chapter-body-dock flex min-h-0 min-w-0 flex-col overflow-hidden"
        :style="{ gridArea: 'chat', ...paperSchemeStyle(paperScheme) }"
        :class="chatDock === 'right' ? 'border-l' : 'border-t'"
      >
        <div class="chapter-body-dock-bar flex shrink-0 flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
          <div class="flex min-w-0 flex-wrap items-center gap-2">
            <div class="chapter-body-dock-title min-w-0 truncate text-sm font-medium">
              {{ t("workspace.bodyDockTitle") }}
            </div>
            <span
              v-if="selectedIsChapter"
              class="chapter-body-dock-muted shrink-0 text-xs tabular-nums"
            >{{ t("workspace.wordWritten", { n: bodyWordCount }) }}</span>
            <div
              v-if="bodyDiffReady"
              class="chapter-body-dock-pos inline-flex rounded-md border p-0.5"
              role="tablist"
              :aria-label="t('workspace.refineDiffTab')"
            >
              <button
                type="button"
                class="rounded px-1.5 py-1 text-xs transition-colors"
                :class="bodyView === 'edit' ? 'is-on' : ''"
                :aria-selected="bodyView === 'edit'"
                @click="bodyView = 'edit'"
              >{{ t("workspace.bodyTab") }}</button>
              <button
                type="button"
                class="rounded px-1.5 py-1 text-xs transition-colors"
                :class="bodyView === 'diff' ? 'is-on' : ''"
                :aria-selected="bodyView === 'diff'"
                @click="bodyView = 'diff'"
              >{{ t("workspace.refineDiffTab") }}</button>
            </div>
          </div>
          <div class="flex min-w-0 flex-wrap items-center justify-end gap-2">
            <template v-if="selectedIsChapter">
              <Button
                v-if="chapterBusy"
                size="sm"
                variant="destructive"
                class="chapter-body-tool-stop"
                @click="stopChapter"
              >
                <Square class="mr-1.5 h-3.5 w-3.5" />
                {{ t("workspace.stop") }}
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="chapter-body-tool-btn px-2"
                :aria-pressed="bodySuggestOn"
                :title="bodySuggestOn ? t('workspace.bodySuggestOn') : t('workspace.bodySuggestOff')"
                :aria-label="bodySuggestOn ? t('workspace.bodySuggestOn') : t('workspace.bodySuggestOff')"
                @click="toggleBodySuggest"
              >
                <Loader2 v-if="bodySuggestBusy" class="h-3.5 w-3.5 animate-spin" />
                <Lightbulb v-else class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="chapter-body-tool-btn px-2"
                :aria-pressed="bodyTts !== 'idle'"
                :disabled="
                  bodyTts === 'idle' &&
                  (!!busy || !(bodyEditing ? bodyDraft : chapterMd).trim())
                "
                :title="
                  bodyTts === 'idle'
                    ? t('workspace.readAloud')
                    : t('workspace.readAloudStop')
                "
                :aria-label="
                  bodyTts === 'idle'
                    ? t('workspace.readAloud')
                    : t('workspace.readAloudStop')
                "
                @click="toggleChapterTts"
              >
                <Loader2 v-if="bodyTts === 'loading'" class="h-3.5 w-3.5 animate-spin" />
                <Pause v-else-if="bodyTts === 'playing'" class="h-3.5 w-3.5" />
                <Play v-else class="h-3.5 w-3.5" />
              </Button>
              <select
                v-model.number="ttsSpeed"
                class="chapter-body-tool-select h-8 rounded-md border px-1.5 text-xs outline-none disabled:opacity-50"
                :disabled="bodyTts !== 'idle'"
                :title="t('workspace.readAloudSpeed')"
                :aria-label="t('workspace.readAloudSpeed')"
              >
                <option :value="1">1×</option>
                <option :value="2">2×</option>
                <option :value="3">3×</option>
              </select>
              <Button
                size="sm"
                variant="outline"
                class="chapter-body-tool-btn px-2"
                :disabled="!bodyEditing || bodyView !== 'edit'"
                :title="t('workspace.findBody')"
                :aria-label="t('workspace.findBody')"
                @click="bodyEditor?.openFind()"
              >
                <Search class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="chapter-body-tool-btn px-2"
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
                class="chapter-body-tool-btn px-2"
                :disabled="shotsBusy || (chapterBusy && !['split-shots','shot-prompts','submit-comfy'].includes(busy))"
                :title="shotsPreviewTitle"
                :aria-label="shotsPreviewTitle"
                @click="openChapterShots"
              >
                <Loader2
                  v-if="shotsBusy || ['split-shots','shot-prompts','submit-comfy'].includes(busy)"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <Clapperboard v-else class="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="outline"
                class="chapter-body-tool-btn px-2"
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
              <span v-if="copyHint" class="chapter-body-dock-muted text-[11px]">{{ copyHint }}</span>
            </template>
            <div
              class="chapter-body-dock-pos inline-flex rounded-md border p-0.5"
              role="group"
              :aria-label="t('workspace.bodyDockHint')"
            >
              <button
                type="button"
                class="rounded px-1.5 py-1 transition-colors"
                :class="chatDock === 'right' ? 'is-on' : ''"
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
                :class="chatDock === 'bottom' ? 'is-on' : ''"
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
        <div
          class="flex min-h-0 flex-1 flex-col overflow-hidden"
          :class="selectedIsChapter ? '' : 'p-3'"
        >
          <template v-if="!selectedIsChapter">
            <p class="chapter-body-dock-muted text-xs">{{ t("workspace.bodyDockPickChapter") }}</p>
          </template>
          <template v-else>
            <div class="flex min-h-0 min-w-0 w-full flex-1 flex-col gap-2 overflow-hidden">
            <div
              v-if="bodyView === 'diff' && bodyDiffReady"
              class="chapter-body-diff min-h-0 flex-1 overflow-y-auto px-3 py-2"
            >
              <p class="chapter-body-dock-muted mb-2 text-[11px]">{{ t("workspace.refineDiffHint") }}</p>
              <div class="font-sans text-sm leading-relaxed">
                <template v-for="(b, i) in bodyDiffBlocks" :key="i">
                  <div
                    v-if="b.type === 'eq'"
                    class="whitespace-pre-wrap px-1"
                  >{{ b.lines.join("\n") || "\u00a0" }}</div>
                  <div
                    v-else
                    class="body-diff-conflict mb-2 overflow-hidden rounded-md border"
                  >
                    <div class="body-diff-conflict-bar flex flex-wrap items-center gap-1 px-1.5 py-1">
                      <button
                        type="button"
                        class="rounded px-1.5 py-0.5 text-[11px]"
                        :disabled="bodySaveBusy"
                        @click="pickBodyDiff(i, 'old')"
                      >{{ t("workspace.refineDiffKeepBefore") }}</button>
                      <button
                        type="button"
                        class="rounded px-1.5 py-0.5 text-[11px]"
                        :disabled="bodySaveBusy"
                        @click="pickBodyDiff(i, 'new')"
                      >{{ t("workspace.refineDiffKeepAfter") }}</button>
                    </div>
                    <div
                      class="body-diff-del whitespace-pre-wrap px-1"
                    >{{ b.oldLines.length ? b.oldLines.join("\n") : t("workspace.refineDiffEmpty") }}</div>
                    <div
                      class="body-diff-add whitespace-pre-wrap px-1"
                    >{{ b.newLines.length ? b.newLines.join("\n") : t("workspace.refineDiffEmpty") }}</div>
                  </div>
                </template>
              </div>
            </div>
            <ChapterBodyEditor
              v-else-if="bodyEditing"
              ref="bodyEditor"
              v-model="bodyDraft"
              :placeholder="t('workspace.editBodyPh')"
              :disabled="bodySaveBusy || paraRewriteBusy"
              :nouns="bodyNouns"
              :rewrite-label="t('workspace.paraRewrite')"
              :rewrite-disabled="paraRewriteBusy || bodySaveBusy"
              :aria-label="t('workspace.editBody')"
              @rewrite="openParaRewrite"
            />
            <div
              v-if="bodySuggestOn"
              class="chapter-body-suggest max-h-36 shrink-0 overflow-y-auto rounded-md border px-2 py-1.5"
            >
              <p class="chapter-body-dock-muted mb-1 text-[11px]">{{ t("workspace.bodySuggestHint") }}</p>
              <div v-if="bodySuggestBusy && !bodySuggestItems.length" class="chapter-body-dock-muted flex items-center gap-2 text-xs">
                <Loader2 class="h-3.5 w-3.5 animate-spin" />
                {{ t("workspace.bodySuggestBusy") }}
              </div>
              <p v-else-if="bodySuggestError && !bodySuggestItems.length" class="text-xs text-destructive">
                {{ bodySuggestError }}
              </p>
              <ol v-else-if="bodySuggestItems.length" class="space-y-1">
                <li v-for="(item, i) in bodySuggestItems" :key="i">
                  <button
                    type="button"
                    class="chapter-body-suggest-item w-full rounded px-1.5 py-1 text-left text-xs leading-relaxed"
                    @click="applyBodySuggest(item)"
                  >
                    {{ i + 1 }}. {{ item }}
                  </button>
                </li>
              </ol>
            </div>
            </div>
          </template>
        </div>
        <div
          v-if="selectedIsChapter && bodyEditing && (bodySaveBusy || bodyAutosaved)"
          class="chapter-body-dock-bar flex shrink-0 items-center border-t px-3 py-1.5"
        >
          <span class="chapter-body-dock-muted text-[11px]">{{
            bodySaveBusy ? t("workspace.bodySaving") : t("workspace.bodyAutosaved")
          }}</span>
        </div>
      </div>
    </div>

    <!-- 朗读模型下载进度 -->
    <div
      v-if="bodyTtsDownload"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.readAloudModelTitle") }}</h2>
        <div class="mt-3 space-y-2">
          <div class="flex items-center justify-between gap-2 text-sm">
            <span>{{ ttsDownloadLabel }}</span>
            <span class="tabular-nums text-muted-foreground">{{ ttsDownloadPercent }}%</span>
          </div>
          <div class="h-2 overflow-hidden rounded-full bg-muted">
            <div
              class="h-full rounded-full bg-primary transition-[width] duration-200"
              :style="{ width: `${ttsDownloadPercent}%` }"
            />
          </div>
          <p v-if="ttsDownloadDetail" class="truncate text-xs text-muted-foreground">
            {{ ttsDownloadDetail }}
          </p>
        </div>
        <div class="mt-4 flex justify-end">
          <Button size="sm" variant="outline" @click="stopChapterTts">
            {{ t("novels.cancel") }}
          </Button>
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

    <!-- 细纲单条 AI 重写提示词 -->
    <div
      v-if="pendingDetailedOutlineAi"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelDetailedOutlineAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">
          {{ t("workspace.chapterDetailedOutlineItemAiTitle") }}
        </h2>
        <p class="mt-2 text-xs text-muted-foreground">
          {{ t("workspace.chapterDetailedOutlineItemAiHint") }}
        </p>
        <p
          class="mt-2 max-h-28 overflow-y-auto whitespace-pre-wrap rounded border bg-muted/40 px-2 py-1.5 text-xs text-muted-foreground"
        >
          {{ pendingDetailedOutlineAi.text.trim() || t("workspace.chapterDetailedOutlineItemAiEmpty") }}
        </p>
        <label class="mt-3 block text-xs text-muted-foreground">{{
          t("workspace.chapterDetailedOutlineAiNote")
        }}</label>
        <Textarea
          v-model="detailedOutlineAiNote"
          rows="4"
          class="mt-1 text-sm"
          :disabled="detailedOutlineItemBusy !== null"
          :placeholder="t('workspace.chapterDetailedOutlineItemAiNotePh')"
        />
        <p v-if="detailedOutlineAiError" class="mt-2 text-xs text-destructive">{{ detailedOutlineAiError }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button
            size="sm"
            variant="outline"
            :disabled="detailedOutlineItemBusy !== null"
            @click="cancelDetailedOutlineAi"
          >
            {{ t("novels.cancel") }}
          </Button>
          <Button
            size="sm"
            :disabled="detailedOutlineItemBusy !== null"
            @click="confirmDetailedOutlineAi"
          >
            <Loader2
              v-if="detailedOutlineItemBusy !== null"
              class="mr-1.5 h-3.5 w-3.5 animate-spin"
            />
            <Sparkles v-else class="mr-1.5 h-3.5 w-3.5" />
            {{
              detailedOutlineItemBusy !== null
                ? t("workspace.chapterDetailedOutlineAiBusy")
                : t("workspace.chapterDetailedOutlineAiRun")
            }}
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
      :slot="worldviewChatSlot"
      @close="worldviewChatOpen = false"
      @applied="onWorldviewChatApplied"
    />
    <StoryRulesChatPanel
      :open="storyRulesChatOpen"
      :novel-id="props.id"
      @close="storyRulesChatOpen = false"
      @applied="onStoryRulesChatApplied"
    />

    <div
      v-if="publicPickOpen"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="closePublicPick"
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
            class="flex w-full flex-col items-start gap-0.5 border-b border-border/50 px-3 py-2 text-left last:border-b-0 hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="publicPickBusy || publicCardAlreadyOnTree(c)"
            @click="addPickedPublicCard(c.id)"
          >
            <span class="flex w-full items-center justify-between gap-2 text-sm font-medium">
              <span class="min-w-0 truncate">{{ c.title }}</span>
              <span
                v-if="publicCardAlreadyOnTree(c)"
                class="shrink-0 text-[10px] font-normal text-muted-foreground"
              >
                {{ t("workspace.pickPublicKnowledgeAlreadyAdded") }}
              </span>
            </span>
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
          <Button variant="outline" :disabled="publicPickBusy" @click="closePublicPick">
            {{ t("novels.cancel") }}
          </Button>
        </div>
      </div>
    </div>

    <div
      v-if="shotsPanelOpen"
      class="fixed inset-0 z-50 flex items-stretch justify-center bg-black/30 p-0 sm:p-4 md:p-6"
      @click.self="closeChapterShots"
      @keydown.escape="closeChapterShots"
    >
      <div
        class="flex h-full w-full max-w-4xl flex-col border bg-background shadow-lg sm:h-[min(92vh,860px)] sm:rounded-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="shotsPanelHeading"
      >
        <div class="flex shrink-0 items-center justify-between gap-2 border-b px-4 py-3">
          <div class="min-w-0">
            <h2 class="truncate text-sm font-semibold">{{ shotsPanelHeading }}</h2>
            <p class="truncate text-xs text-muted-foreground">{{ selected?.label }}</p>
          </div>
          <Button size="sm" variant="ghost" class="px-2" @click="closeChapterShots">
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          <p class="text-xs text-muted-foreground">{{ t("workspace.shotsHint") }}</p>
          <p v-if="shotsError" class="text-xs text-destructive">{{ shotsError }}</p>
          <p v-if="shotsNotice" class="text-xs text-primary">{{ shotsNotice }}</p>
          <div v-if="shotsBusy" class="flex items-center gap-2 text-xs text-muted-foreground">
            <Loader2 class="h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.shotsBusy") }}
          </div>
          <p v-else-if="!shots.length" class="text-xs text-muted-foreground">
            {{ t("workspace.shotsEmpty") }}
          </p>
          <article
            v-for="(s, i) in shots"
            :key="s.id || i"
            class="space-y-2 rounded-md border p-3"
          >
            <div class="flex items-center justify-between gap-2">
              <p class="text-xs font-medium text-muted-foreground">#{{ s.order || i + 1 }}</p>
              <Button
                size="sm"
                variant="ghost"
                class="h-7 px-2 text-muted-foreground"
                :disabled="!!busy"
                :aria-label="t('workspace.shotsRemove')"
                @click="removeShot(i)"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </Button>
            </div>
            <label class="block text-xs text-muted-foreground">{{ t("workspace.shotsAction") }}</label>
            <textarea
              v-model="s.action"
              rows="2"
              class="w-full rounded-md border bg-background px-2 py-1 text-sm"
              @change="persistShots"
            />
            <div class="grid gap-2 sm:grid-cols-2">
              <div>
                <label class="block text-xs text-muted-foreground">{{ t("workspace.shotsCamera") }}</label>
                <input
                  v-model="s.camera"
                  class="mt-0.5 h-8 w-full rounded-md border px-2 text-sm"
                  @change="persistShots"
                />
              </div>
              <div>
                <label class="block text-xs text-muted-foreground">{{ t("workspace.shotsDuration") }}</label>
                <input
                  v-model.number="s.duration_sec"
                  type="number"
                  min="5"
                  max="15"
                  class="mt-0.5 h-8 w-24 rounded-md border px-2 text-sm"
                  @change="persistShots"
                />
              </div>
            </div>
            <label class="block text-xs text-muted-foreground">{{ t("workspace.shotsDialogue") }}</label>
            <input
              v-model="s.dialogue"
              class="h-8 w-full rounded-md border px-2 text-sm"
              @change="persistShots"
            />
            <label class="block text-xs text-muted-foreground">{{ t("workspace.shotsPrompt") }}</label>
            <textarea
              v-model="s.comfy_prompt"
              rows="3"
              class="w-full rounded-md border bg-background px-2 py-1 font-mono text-xs"
              @change="persistShots"
            />
          </article>
        </div>
        <div class="flex shrink-0 flex-wrap justify-end gap-2 border-t px-4 py-3">
          <Button size="sm" variant="ghost" :disabled="!!busy" @click="addShot">
            <Plus class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.shotsAdd") }}
          </Button>
          <Button size="sm" variant="outline" :disabled="!!busy" @click="requestSplitShots">
            <Loader2 v-if="busy === 'split-shots'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.shotsSplit") }}
          </Button>
          <Button
            size="sm"
            variant="outline"
            :disabled="!!busy || !shots.length"
            @click="runShotPrompts"
          >
            <Loader2 v-if="busy === 'shot-prompts'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.shotsPrompts") }}
          </Button>
          <Button
            size="sm"
            :disabled="!!busy || !shots.length"
            @click="runSubmitComfy"
          >
            <Loader2 v-if="busy === 'submit-comfy'" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.shotsSubmit") }}
          </Button>
        </div>
      </div>
    </div>

    <div
      v-if="pendingShotSplit"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="pendingShotSplit = null"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-4 shadow-lg" role="dialog">
        <h3 class="text-sm font-semibold">{{ t("workspace.shotsOverwriteTitle") }}</h3>
        <p class="mt-2 text-sm text-muted-foreground">{{ t("workspace.shotsOverwriteBody") }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="ghost" @click="pendingShotSplit = null">
            {{ t("workspace.shotsCancel") }}
          </Button>
          <Button
            v-if="pendingShotSplit === 1"
            size="sm"
            @click="pendingShotSplit = 2"
          >
            {{ t("workspace.shotsOverwriteContinue") }}
          </Button>
          <Button
            v-else
            size="sm"
            variant="destructive"
            @click="runSplitShots"
          >
            {{ t("workspace.shotsOverwriteConfirm") }}
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

    <!-- 章节总字数 -->
    <div
      v-if="wordStatsOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4"
      @click.self="closeWordStats"
      @keydown.escape="closeWordStats"
    >
      <div
        class="flex max-h-[min(85vh,32rem)] w-full max-w-md flex-col rounded-lg border bg-background p-5 shadow-lg"
        role="dialog"
        aria-modal="true"
        :aria-label="t('workspace.wordStats')"
      >
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <h2 class="text-base font-semibold">{{ t("workspace.wordStats") }}</h2>
            <p class="mt-1 text-xs text-muted-foreground">{{ t("workspace.wordStatsHint") }}</p>
          </div>
          <Button
            size="sm"
            variant="ghost"
            class="shrink-0 px-2"
            :disabled="wordStatsBusy"
            :aria-label="t('workspace.memoryClose')"
            @click="closeWordStats"
          >
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="mt-4 space-y-3">
          <label class="flex items-center gap-2 text-sm">
            <input v-model="wordStatsMode" type="radio" value="all" class="h-4 w-4" :disabled="wordStatsBusy" />
            {{ t("workspace.wordStatsAll") }}
          </label>
          <label class="flex flex-wrap items-center gap-2 text-sm">
            <input v-model="wordStatsMode" type="radio" value="range" class="h-4 w-4" :disabled="wordStatsBusy" />
            {{ t("workspace.wordStatsRange") }}
            <span class="text-muted-foreground">{{ t("workspace.wordStatsFrom") }}</span>
            <Input
              v-model.number="wordStatsFrom"
              type="number"
              min="1"
              class="h-8 w-16"
              :disabled="wordStatsBusy || wordStatsMode !== 'range'"
            />
            <span class="text-muted-foreground">{{ t("workspace.wordStatsTo") }}</span>
            <Input
              v-model.number="wordStatsTo"
              type="number"
              min="1"
              class="h-8 w-16"
              :disabled="wordStatsBusy || wordStatsMode !== 'range'"
            />
            <span class="text-muted-foreground">{{ t("workspace.wordStatsChapter") }}</span>
          </label>
        </div>
        <p v-if="wordStatsError" class="mt-3 text-xs text-destructive">{{ wordStatsError }}</p>
        <div v-if="wordStatsResult" class="mt-3 min-h-0 flex-1 space-y-2 overflow-hidden">
          <p class="text-sm font-medium tabular-nums">
            {{
              t("workspace.wordStatsTotal", {
                n: wordStatsResult.items.length,
                words: wordStatsResult.total.toLocaleString(),
              })
            }}
          </p>
          <ul class="max-h-40 overflow-y-auto rounded-md border bg-muted/30 p-2 text-xs">
            <li v-for="(row, i) in wordStatsResult.items" :key="i" class="flex justify-between gap-2 py-0.5">
              <span class="min-w-0 truncate">{{ row.label }}</span>
              <span class="shrink-0 tabular-nums text-muted-foreground">
                {{ t("workspace.wordStatsItem", { n: row.words.toLocaleString() }) }}
              </span>
            </li>
          </ul>
        </div>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="wordStatsBusy" @click="closeWordStats">
            {{ t("novels.cancel") }}
          </Button>
          <Button :disabled="wordStatsBusy" @click="runWordStats">
            {{ wordStatsBusy ? t("workspace.wordStatsRunning") : t("workspace.wordStatsRun") }}
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

<style scoped>
.chapter-body-dock {
  --cb-paper: #ffffff;
  --cb-ink: #111111;
  --cb-muted: #737373;
  --cb-border: #d4d4d4;
  --cb-wash: rgba(0, 0, 0, 0.06);
  --cb-panel: #fafafa;
  background-color: var(--cb-paper);
  border-color: var(--cb-border);
  color: var(--cb-ink);
}
.chapter-body-dock-bar {
  border-color: var(--cb-border);
}
.chapter-body-dock-title {
  color: var(--cb-ink);
}
.chapter-body-dock-muted {
  color: var(--cb-muted);
}
.chapter-body-dock :deep(.chapter-body-tool-btn) {
  border-color: var(--cb-border);
  background-color: transparent;
  color: var(--cb-ink);
}
.chapter-body-dock :deep(.chapter-body-tool-btn:hover:not(:disabled)) {
  background-color: var(--cb-wash);
}
.chapter-body-dock :deep(.chapter-body-tool-btn[aria-pressed="true"]) {
  border-color: var(--cb-ink);
  background-color: var(--cb-wash);
  color: var(--cb-ink);
}
.chapter-body-dock :deep(.chapter-body-tool-stop) {
  background-color: var(--cb-ink);
  color: var(--cb-paper);
}
.chapter-body-dock :deep(.chapter-body-tool-stop:hover:not(:disabled)) {
  background-color: var(--cb-ink);
  opacity: 0.9;
}
.chapter-body-tool-select {
  border-color: var(--cb-border);
  background-color: transparent;
  color: var(--cb-ink);
}
.chapter-body-tool-select:hover:not(:disabled) {
  background-color: var(--cb-wash);
}
.chapter-body-dock-pos {
  border-color: var(--cb-border);
  background-color: var(--cb-wash);
}
.chapter-body-dock-pos button {
  color: var(--cb-muted);
}
.chapter-body-dock-pos button:hover {
  color: var(--cb-ink);
  background-color: var(--cb-wash);
}
.chapter-body-diff .body-diff-del {
  background: #fecaca;
  color: #7f1d1d;
  text-decoration: line-through;
}
.chapter-body-diff .body-diff-add {
  background: #bbf7d0;
  color: #14532d;
}
.chapter-body-diff .body-diff-conflict {
  border-color: var(--cb-border);
}
.chapter-body-diff .body-diff-conflict-bar {
  background-color: var(--cb-wash);
  color: var(--cb-muted);
}
.chapter-body-diff .body-diff-conflict-bar button:hover:not(:disabled) {
  color: var(--cb-ink);
  background-color: var(--cb-paper);
}
.chapter-body-diff .body-diff-conflict-bar button:disabled {
  opacity: 0.5;
}
.chapter-body-dock-pos button.is-on {
  background-color: var(--cb-ink);
  color: var(--cb-paper);
}
.chapter-body-suggest {
  border-color: var(--cb-border);
  background-color: color-mix(in srgb, var(--cb-ink) 6%, var(--cb-paper));
  color: var(--cb-ink);
}
.chapter-body-suggest-item:hover {
  background-color: var(--cb-wash);
}
</style>
