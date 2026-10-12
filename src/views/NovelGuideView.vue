<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useLocalStorage } from "@vueuse/core";
import { useRouter } from "vue-router";
import { Loader2, Square } from "@lucide/vue";
import { api, type NovelTree, type TreeNode } from "@/lib/api";
import { useI18n, type MessageKey } from "@/i18n";
import { usePersistedChatModel } from "@/lib/chatModel";
import { applyWorldviewPayload } from "@/lib/worldviewGen";
import { applyStoryRulesPayload } from "@/lib/storyRulesGen";
import { plainTree } from "@/lib/plainTree";
import { emptyNovelFeatures, normalizeNovelFeatures, type NovelFeatures } from "@/lib/novelFeatures";
import {
  knowledgeSlot,
  STORY_RULES_SLOT,
  WORLDVIEW_FAN_SLOTS,
  isStoryRulesSlot,
  worldviewFanTitleKey,
} from "@/lib/worldview";
import {
  STORY_RULES_FAN_SLOTS,
  isStoryRulesFanSlot,
  normalizeConstraintRedlines,
  normalizeFulfillmentSystem,
  normalizeStoryEngine,
  normalizeSurfaceSetting,
  storyRulesFanTitleKey,
  type StoryRulesBlockSlot,
} from "@/lib/storyRules";
import { findStoryRulesNode, syncStoryRulesParentExtracted } from "@/lib/storyRulesCards";
import { formatCharacterExtracted } from "@/lib/characterCard";
import { formatVolumeExtracted, normalizeVolume, parseVolumeFromFormatted } from "@/lib/volume";
import { volumeChapterIds } from "@/lib/treeLayout";
import { joinGuideMarkdown, splitGuideMarkdown } from "@/lib/guideMarkdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import NovelFeaturesPicker from "@/components/NovelFeaturesPicker.vue";

type Step = "seed" | "worldview" | "rules" | "cast" | "structure";

const STEPS: { id: Step; title: MessageKey }[] = [
  { id: "seed", title: "novels.guide.stepSeed" },
  { id: "worldview", title: "novels.guide.stepWorldview" },
  { id: "rules", title: "novels.guide.stepRules" },
  { id: "cast", title: "novels.guide.stepCast" },
  { id: "structure", title: "novels.guide.stepStructure" },
];

const { t } = useI18n();
const router = useRouter();
const lastNovelId = useLocalStorage("novework.lastNovelId", "");
const { model, options, load: loadModel, persist: persistModel } = usePersistedChatModel("chat_model");

const step = ref<Step>("seed");
const novelId = ref("");
const title = ref("");
const synopsis = ref("");
const features = ref<NovelFeatures>(emptyNovelFeatures());
const wordMin = ref(2000);
const wordMax = ref(3000);
const chapterCount = ref(20);
const notes = ref("");
const busy = ref(false);
const error = ref("");
const reply = ref("");
const previewTree = ref<NovelTree | null>(null);
const mdDraft = ref("");
const mdDirty = ref(false);

type PreviewCard = {
  id: string;
  title: string;
  body: string;
  kind: string;
  slot: string;
  children: PreviewCard[];
};

const WV_CHILD_SLOTS = new Set([
  "wv_axiom",
  "wv_location",
  "wv_race",
  "wv_faction",
  "wv_religion",
  "wv_major_event",
]);

function neighbors(tree: NovelTree, hostId: string): TreeNode[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const ids = new Set<string>();
  const host = byId.get(hostId);
  for (const id of host?.linked_knowledge_ids ?? []) ids.add(id);
  for (const id of host?.linked_character_ids ?? []) ids.add(id);
  for (const e of tree.edges) {
    const other = e.source === hostId ? e.target : e.target === hostId ? e.source : null;
    if (other) ids.add(other);
  }
  ids.delete(hostId);
  const out: TreeNode[] = [];
  for (const id of ids) {
    const n = byId.get(id);
    if (n) out.push(n);
  }
  out.sort(
    (a, b) =>
      a.position.y - b.position.y || a.position.x - b.position.x || a.id.localeCompare(b.id),
  );
  return out;
}

function cardTitle(n: TreeNode): string {
  const slot = knowledgeSlot(n);
  const wv = worldviewFanTitleKey(slot);
  if (wv) return t(wv);
  const sr = storyRulesFanTitleKey(slot);
  if (sr) return t(sr);
  if (isStoryRulesSlot(slot)) return t(STORY_RULES_SLOT.titleKey);
  return n.label.trim() || t("workspace.selectNode");
}

function cardBody(n: TreeNode): string {
  if (n.kind === "character") {
    const dumped = n.character?.constraints?.trim();
    if (dumped) return dumped;
    if (n.character) return formatCharacterExtracted(n.character, n.label).trim() || n.outline;
    return n.outline;
  }
  if (n.kind === "volume") {
    return formatVolumeExtracted(normalizeVolume(n.volume)) || n.outline.trim();
  }
  if (n.kind === "novel") {
    const bits = [
      n.outline.trim(),
      n.word_count_min || n.word_count_max
        ? t("novels.wordsPerChapter", { min: n.word_count_min, max: n.word_count_max })
        : "",
      n.chapter_count ? t("novels.chapterCount", { n: n.chapter_count }) : "",
    ];
    return bits.filter(Boolean).join("\n");
  }
  return (n.knowledge?.extracted || n.outline || "").trim();
}

function toCard(n: TreeNode, children: PreviewCard[] = []): PreviewCard {
  return {
    id: n.id,
    title: cardTitle(n),
    body: cardBody(n),
    kind: n.kind,
    slot: knowledgeSlot(n),
    children,
  };
}

function buildPreview(tree: NovelTree, s: Step): PreviewCard[] {
  if (s === "seed") {
    const root = tree.nodes.find((n) => n.kind === "novel");
    return root ? [toCard(root)] : [];
  }
  if (s === "worldview") {
    const fans = WORLDVIEW_FAN_SLOTS.map((d) =>
      tree.nodes.find((n) => n.kind === "knowledge" && knowledgeSlot(n) === d.slot),
    ).filter((n): n is TreeNode => !!n);
    return fans.map((fan) =>
      toCard(
        fan,
        neighbors(tree, fan.id)
          .filter((c) => c.kind === "knowledge" && WV_CHILD_SLOTS.has(knowledgeSlot(c)))
          .map((c) => toCard(c)),
      ),
    );
  }
  if (s === "rules") {
    return STORY_RULES_FAN_SLOTS.map((d) =>
      tree.nodes.find((n) => n.kind === "knowledge" && knowledgeSlot(n) === d.slot),
    )
      .filter((n): n is TreeNode => !!n)
      .map((n) => toCard(n));
  }
  if (s === "cast") {
    const root = tree.nodes.find((n) => n.kind === "novel");
    if (!root) return [];
    return neighbors(tree, root.id)
      .filter((n) => n.kind === "character")
      .map((n) => toCard(n));
  }
  const volumes = tree.nodes
    .filter((n) => n.kind === "volume")
    .sort((a, b) => a.position.y - b.position.y || a.position.x - b.position.x);
  const listed = new Set<string>();
  const out: PreviewCard[] = [];
  for (const vol of volumes) {
    const chIds = volumeChapterIds(vol.id, tree.nodes, tree.edges);
    const chs = chIds
      .map((id) => tree.nodes.find((n) => n.id === id))
      .filter((n): n is TreeNode => !!n);
    for (const c of chs) listed.add(c.id);
    out.push(toCard(vol, chs.map((c) => toCard(c))));
  }
  for (const ch of tree.nodes.filter((n) => n.kind === "chapter")) {
    if (listed.has(ch.id)) continue;
    out.push(toCard(ch));
  }
  return out;
}

function flattenCards(cards: PreviewCard[]): PreviewCard[] {
  const out: PreviewCard[] = [];
  for (const c of cards) {
    out.push(c);
    if (c.children.length) out.push(...flattenCards(c.children));
  }
  return out;
}

function cardsToMarkdown(cards: PreviewCard[]): string {
  return joinGuideMarkdown(flattenCards(cards).map((c) => ({ title: c.title, body: c.body })));
}

function h2Map(md: string): Record<string, string> {
  const map: Record<string, string> = {};
  let cur = "";
  let buf: string[] = [];
  const flush = () => {
    if (cur) map[cur] = buf.join("\n").trim();
  };
  for (const line of md.split("\n")) {
    if (line.startsWith("## ") || line.startsWith("### ")) {
      flush();
      cur = line.replace(/^#+\s*/, "").trim();
      buf = [];
    } else {
      buf.push(line);
    }
  }
  flush();
  return map;
}

function listFromMd(s: string): string[] {
  const rows = s
    .split("\n")
    .map((l) => l.replace(/^\d+\.\s*/, "").replace(/^[-*]\s*/, "").trim())
    .filter(Boolean);
  return rows.length ? rows : [""];
}

function take(m: Record<string, string>, key: string, fallback: string): string {
  return m[key] != null && m[key] !== "" ? m[key] : fallback;
}

function writeStoryRulesFields(n: TreeNode, body: string) {
  const k = n.knowledge;
  if (!k) return;
  const slot = knowledgeSlot(n) as StoryRulesBlockSlot;
  const m = h2Map(body);
  if (slot === "sr_surface_setting") {
    k.surface_setting = normalizeSurfaceSetting({
      ...k.surface_setting,
      premise: take(m, "一句话立意", k.surface_setting?.premise ?? ""),
      core_conflict: take(m, "核心冲突", k.surface_setting?.core_conflict ?? ""),
      reader_promise: take(m, "读者承诺", k.surface_setting?.reader_promise ?? ""),
      target_audience: take(m, "目标读者", k.surface_setting?.target_audience ?? ""),
      tone_reference: take(m, "基调参照", k.surface_setting?.tone_reference ?? ""),
      commercial_tags: take(m, "商业标签", k.surface_setting?.commercial_tags ?? ""),
      extended_premise: take(m, "扩展立意", k.surface_setting?.extended_premise ?? ""),
    });
  } else if (slot === "sr_story_engine") {
    k.story_engine = normalizeStoryEngine({
      ...k.story_engine,
      premise: take(m, "一句话立意", k.story_engine?.premise ?? ""),
      bright_line: take(m, "明线", k.story_engine?.bright_line ?? ""),
      dark_line: take(m, "暗线", k.story_engine?.dark_line ?? ""),
      suspense_setup: take(m, "悬念设定", k.story_engine?.suspense_setup ?? ""),
      conflict_engine: take(m, "冲突引擎", k.story_engine?.conflict_engine ?? ""),
      external_conflict: take(m, "外部", k.story_engine?.external_conflict ?? ""),
      internal_conflict: take(m, "内部", k.story_engine?.internal_conflict ?? ""),
      relational_conflict: take(m, "关系", k.story_engine?.relational_conflict ?? ""),
      progression_cycle: take(m, "推进循环", k.story_engine?.progression_cycle ?? ""),
      protagonist_dilemma: take(m, "主角困局", k.story_engine?.protagonist_dilemma ?? ""),
    });
  } else if (slot === "sr_fulfillment_system") {
    k.fulfillment_system = normalizeFulfillmentSystem({
      ...k.fulfillment_system,
      premise: take(m, "一句话立意", k.fulfillment_system?.premise ?? ""),
      growth_path: take(m, "成长路径", k.fulfillment_system?.growth_path ?? ""),
      ending_texture: take(m, "结局质感", k.fulfillment_system?.ending_texture ?? ""),
      payoff_syntax: m["兑现语法"]
        ? listFromMd(m["兑现语法"])
        : k.fulfillment_system?.payoff_syntax,
      emotional_rhythm: take(m, "情绪节奏", k.fulfillment_system?.emotional_rhythm ?? ""),
      tension_archetypes: m["张力原型"]
        ? listFromMd(m["张力原型"])
        : k.fulfillment_system?.tension_archetypes,
    });
  } else if (slot === "sr_constraint_redlines") {
    k.constraint_redlines = normalizeConstraintRedlines({
      ...k.constraint_redlines,
      premise: take(m, "一句话立意", k.constraint_redlines?.premise ?? ""),
      redlines: m["约束红线"] ? listFromMd(m["约束红线"]) : k.constraint_redlines?.redlines,
    });
  }
}

function isFixedTitle(n: TreeNode): boolean {
  const slot = knowledgeSlot(n);
  return !!(worldviewFanTitleKey(slot) || storyRulesFanTitleKey(slot) || isStoryRulesSlot(slot));
}

function writeNode(tree: NovelTree, card: PreviewCard, title: string, body: string) {
  const n = tree.nodes.find((x) => x.id === card.id);
  if (!n) return;
  if (title && !isFixedTitle(n)) n.label = title;
  if (n.kind === "character") {
    n.outline = body.slice(0, 200);
    // ponytail: 引导里只回写 constraints，结构化字段仍以生成结果为准；要反解析 Markdown 再开 parse
    if (n.character) n.character.constraints = body;
    return;
  }
  if (n.kind === "volume") {
    n.volume = parseVolumeFromFormatted(body);
    n.outline = body;
    return;
  }
  if (n.kind === "chapter" || n.kind === "novel") {
    n.outline = body;
    return;
  }
  if (n.knowledge) {
    n.knowledge.extracted = body;
    n.outline = body.slice(0, 200);
    if (isStoryRulesFanSlot(knowledgeSlot(n))) writeStoryRulesFields(n, body);
  }
}

async function flushMarkdown(forStep?: Step, tree?: NovelTree | null) {
  const nid = novelId.value;
  const tr = tree ?? previewTree.value;
  if (!mdDirty.value || !nid || !tr) return;
  const s = forStep ?? step.value;
  const cards = buildPreview(tr, s);
  if (!cards.length) return;
  const flat = flattenCards(cards);
  const used = new Set<string>();
  for (const sec of splitGuideMarkdown(
    mdDraft.value,
    flat.map((c) => c.title),
  )) {
    const byTitle = flat.find((c) => !used.has(c.id) && c.title === sec.title);
    const card = byTitle ?? flat.find((c) => !used.has(c.id));
    if (!card) continue;
    used.add(card.id);
    writeNode(tr, card, sec.title, sec.body);
  }
  if (s === "rules") {
    const hub = findStoryRulesNode(tr);
    if (hub) syncStoryRulesParentExtracted(tr, hub.id, (key) => t(key));
  }
  previewTree.value = await api.saveTreeJson(nid, JSON.stringify(plainTree(tr)));
  mdDirty.value = false;
}

function onMdInput(v: string | number) {
  mdDraft.value = String(v);
  mdDirty.value = true;
}

const previewRoots = computed(() => {
  const tr = previewTree.value;
  if (!tr) return [];
  return buildPreview(tr, step.value);
});

watch(previewRoots, (roots) => {
  if (mdDirty.value) return;
  mdDraft.value = cardsToMarkdown(roots);
});

async function loadPreview() {
  if (!novelId.value) {
    previewTree.value = null;
    mdDraft.value = "";
    mdDirty.value = false;
    return;
  }
  try {
    previewTree.value = await api.getTree(novelId.value);
  } catch {
    previewTree.value = null;
  }
}

const canSkip = computed(() => step.value !== "seed" && !!novelId.value);
const canWorkspace = computed(() => !!novelId.value);

onMounted(() => {
  void loadModel(t("settings.deprecated"));
});

watch([step, novelId], async (_curr, prev) => {
  if (prev && mdDirty.value) {
    await flushMarkdown(prev[0], previewTree.value);
  }
  mdDirty.value = false;
  await loadPreview();
});

onUnmounted(() => {
  if (mdDirty.value) void flushMarkdown();
  if (novelId.value) void api.chatCancel(novelId.value).catch(() => {});
});

function goList() {
  if (busy.value) return;
  void (async () => {
    await flushMarkdown();
    await router.push("/novels");
  })();
}

function setStep(next: Step) {
  if (next !== "seed" && !novelId.value) return;
  step.value = next;
  error.value = "";
}

async function createSeed() {
  if (novelId.value) {
    setStep("worldview");
    return;
  }
  const tTitle = title.value.trim();
  if (!tTitle) {
    error.value = t("novels.needTitle");
    return;
  }
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    const novel = await api.createNovel({
      title: tTitle,
      synopsis: synopsis.value.trim(),
      knowledge_ids: [],
      knowledge_strategy: "",
      features: normalizeNovelFeatures(features.value),
    });
    novelId.value = novel.id;
    lastNovelId.value = novel.id;
    const min = Math.max(1, Number(wordMin.value) || 2000);
    const max = Math.max(min, Number(wordMax.value) || 3000);
    const n = Math.min(100, Math.max(1, Number(chapterCount.value) || 20));
    await api.updateNovelPlan(novel.id, min, max, n);
    previewTree.value = await api.getTree(novel.id);
    setStep("worldview");
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function runWorldview() {
  const nid = novelId.value;
  if (!nid || busy.value) return;
  busy.value = true;
  error.value = "";
  reply.value = "";
  try {
    await flushMarkdown();
    const content = notes.value.trim() || t("novels.guide.wvFillMsg");
    const res = await api.generateWorldviewChat(
      nid,
      [{ role: "user", content }],
      model.value,
    );
    reply.value = res.assistant;
    const tr = await api.getTree(nid);
    const changed = applyWorldviewPayload(tr, res.worldview, (key) => t(key));
    mdDirty.value = false;
    if (changed) {
      previewTree.value = await api.saveTreeJson(nid, JSON.stringify(plainTree(tr)));
    } else {
      previewTree.value = tr;
    }
  } catch (e) {
    const s = String(e);
    if (!s.includes("cancelled")) error.value = s;
  } finally {
    busy.value = false;
  }
}

async function runRules() {
  const nid = novelId.value;
  if (!nid || busy.value) return;
  busy.value = true;
  error.value = "";
  reply.value = "";
  try {
    await flushMarkdown();
    const content = notes.value.trim() || t("novels.guide.srFillMsg");
    const res = await api.generateStoryRulesChat(
      nid,
      [{ role: "user", content }],
      model.value,
    );
    reply.value = res.assistant;
    const tr = await api.getTree(nid);
    const changed = applyStoryRulesPayload(tr, res.blocks, (key) => t(key));
    mdDirty.value = false;
    if (changed) {
      previewTree.value = await api.saveTreeJson(nid, JSON.stringify(plainTree(tr)));
    } else {
      previewTree.value = tr;
    }
  } catch (e) {
    const s = String(e);
    if (!s.includes("cancelled")) error.value = s;
  } finally {
    busy.value = false;
  }
}

async function runCast() {
  const nid = novelId.value;
  if (!nid || busy.value) return;
  busy.value = true;
  error.value = "";
  reply.value = "";
  try {
    await flushMarkdown();
    const res = await api.generateGuideCast(nid, notes.value, model.value);
    reply.value = res.message;
    mdDirty.value = false;
    await loadPreview();
  } catch (e) {
    const s = String(e);
    if (!s.includes("cancelled")) error.value = s;
  } finally {
    busy.value = false;
  }
}

async function runStructure() {
  const nid = novelId.value;
  if (!nid || busy.value) return;
  busy.value = true;
  error.value = "";
  reply.value = "";
  try {
    await flushMarkdown();
    const res = await api.generateGuideStructure(nid, notes.value, model.value);
    reply.value = res.message;
    mdDirty.value = false;
    await loadPreview();
  } catch (e) {
    const s = String(e);
    if (!s.includes("cancelled")) error.value = s;
  } finally {
    busy.value = false;
  }
}

async function stop() {
  if (!novelId.value) return;
  try {
    await api.chatCancel(novelId.value);
  } catch {
    /* ignore */
  }
}

function skip() {
  if (!canSkip.value || busy.value) return;
  void (async () => {
    await flushMarkdown();
    const i = STEPS.findIndex((s) => s.id === step.value);
    if (i < 0 || i >= STEPS.length - 1) {
      await openWorkspace();
      return;
    }
    setStep(STEPS[i + 1]!.id);
  })();
}

async function openWorkspace() {
  if (!novelId.value) return;
  await flushMarkdown();
  lastNovelId.value = novelId.value;
  await router.push(`/novels/${novelId.value}`);
}

function runCurrent() {
  if (step.value === "seed") void createSeed();
  else if (step.value === "worldview") void runWorldview();
  else if (step.value === "rules") void runRules();
  else if (step.value === "cast") void runCast();
  else void runStructure();
}

const hintKey = computed((): MessageKey => {
  if (step.value === "worldview") return "novels.guide.wvHint";
  if (step.value === "rules") return "novels.guide.srHint";
  if (step.value === "cast") return "novels.guide.castHint";
  if (step.value === "structure") return "novels.guide.structureHint";
  return "novels.guide.seedHint";
});
</script>

<template>
  <div class="mx-auto h-full max-w-3xl space-y-5 overflow-y-auto overscroll-contain p-6">
    <div class="flex items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("novels.guide.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t("novels.guide.subtitle") }}</p>
      </div>
      <Button variant="outline" :disabled="busy" @click="goList">{{ t("novels.guide.back") }}</Button>
    </div>

    <div class="flex flex-wrap gap-1.5">
      <button
        v-for="(s, i) in STEPS"
        :key="s.id"
        type="button"
        class="rounded-md px-2.5 py-1 text-xs"
        :class="
          step === s.id
            ? 'bg-accent font-medium'
            : 'text-muted-foreground hover:bg-muted'
        "
        :disabled="busy || (s.id !== 'seed' && !novelId)"
        @click="setStep(s.id)"
      >
        {{ i + 1 }}. {{ t(s.title) }}
      </button>
    </div>

    <Card class="border-primary/20">
      <CardHeader class="pb-2">
        <CardTitle class="text-base">{{ t(STEPS.find((s) => s.id === step)!.title) }}</CardTitle>
        <p class="text-xs text-muted-foreground">{{ t(hintKey) }}</p>
      </CardHeader>
      <CardContent class="space-y-3">
        <template v-if="step === 'seed'">
          <div>
            <label class="mb-1 block text-sm">{{ t("novels.createTitle") }}</label>
            <Input v-model="title" :placeholder="t('novels.createTitlePh')" :disabled="busy" />
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("novels.createSynopsis") }}</label>
            <Textarea v-model="synopsis" rows="4" :placeholder="t('novels.createSynopsisPh')" :disabled="busy" />
          </div>
          <div class="grid grid-cols-3 gap-2">
            <div>
              <label class="mb-1 block text-sm">{{ t("novels.guide.wordMin") }}</label>
              <Input v-model="wordMin" type="number" min="1" :disabled="busy" />
            </div>
            <div>
              <label class="mb-1 block text-sm">{{ t("novels.guide.wordMax") }}</label>
              <Input v-model="wordMax" type="number" min="1" :disabled="busy" />
            </div>
            <div>
              <label class="mb-1 block text-sm">{{ t("novels.guide.chapters") }}</label>
              <Input v-model="chapterCount" type="number" min="1" max="100" :disabled="busy" />
            </div>
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("novels.feat.title") }}</label>
            <NovelFeaturesPicker v-model="features" :disabled="busy" />
          </div>
        </template>
        <template v-else>
          <div v-if="options.length" class="flex items-center gap-2">
            <select
              v-model="model"
              class="h-8 max-w-[16rem] truncate rounded-md border bg-background px-2 text-xs"
              :disabled="busy"
              @change="persistModel"
            >
              <option v-for="m in options" :key="m.id" :value="m.id">{{ m.label }}</option>
            </select>
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("novels.guide.notes") }}</label>
            <Textarea v-model="notes" rows="4" :placeholder="t('novels.guide.notesPh')" :disabled="busy" />
          </div>
        </template>
        <p v-if="reply" class="whitespace-pre-wrap rounded-md bg-muted px-3 py-2 text-sm">{{ reply }}</p>
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
        <div class="flex flex-wrap justify-end gap-2">
          <Button v-if="canWorkspace" variant="outline" :disabled="busy" @click="openWorkspace">
            {{ t("novels.guide.openWorkspace") }}
          </Button>
          <Button v-if="busy" variant="outline" @click="stop">
            <Square class="mr-1 h-3.5 w-3.5 fill-current" />
            {{ t("novels.guide.stop") }}
          </Button>
          <Button :disabled="busy" @click="runCurrent">
            <Loader2 v-if="busy" class="mr-1 h-3.5 w-3.5 animate-spin" />
            {{
              busy
                ? t("novels.guide.busy")
                : step === "seed"
                  ? t("novels.guide.create")
                  : t("novels.guide.generate")
            }}
          </Button>
          <Button v-if="canSkip" variant="outline" :disabled="busy" @click="skip">
            {{ t("novels.guide.skip") }}
          </Button>
        </div>
        <div v-if="step !== 'seed' && (mdDraft || previewRoots.length)" class="space-y-2 border-t pt-3">
          <p class="text-xs text-muted-foreground">{{ t("novels.guide.preview") }}</p>
          <Textarea
            :model-value="mdDraft"
            rows="18"
            class="min-h-[20rem] font-mono text-[13px] leading-relaxed"
            :placeholder="t('novels.guide.previewEmpty')"
            :disabled="busy"
            @update:model-value="onMdInput"
            @blur="() => flushMarkdown()"
          />
        </div>
        <p
          v-else-if="novelId && step !== 'seed' && !busy"
          class="border-t pt-3 text-xs text-muted-foreground"
        >
          {{ t("novels.guide.previewEmpty") }}
        </p>
      </CardContent>
    </Card>
  </div>
</template>
