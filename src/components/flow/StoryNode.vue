<script setup lang="ts">
import { computed, inject, type Component } from "vue";
import { Handle, Position, type NodeProps } from "@vue-flow/core";
import { BookOpen, ChevronDown, ChevronRight, GitBranch, Layers, Library, User } from "@lucide/vue";
import type { TreeNode } from "@/lib/api";
import { useI18n } from "@/i18n";
import { isStoryRulesSlot, STORY_RULES_SLOT, worldviewFanTitleKey } from "@/lib/worldview";
import { storyRulesFanTitleKey } from "@/lib/storyRules";
import { worldviewFanVisual } from "@/lib/worldviewStyle";
import { volumeDisplaySummary } from "@/lib/volume";

/** 根节点可附带封面 URL；分卷卡可带折叠状态 */
type StoryNodeData = TreeNode & {
  cover_url?: string;
  volumeCollapsed?: boolean;
  collapsedChapterCount?: number;
};

const props = defineProps<NodeProps<StoryNodeData>>();
const { t } = useI18n();

const n = computed(() => props.data);
const coverUrl = computed(() => n.value?.cover_url?.trim() || "");
const kind = computed(() => n.value?.kind ?? "chapter");
const knowledgeSlot = computed(() => (n.value?.knowledge?.slot ?? "").trim());
const isRaceCard = computed(
  () => kind.value === "knowledge" && knowledgeSlot.value === "wv_race",
);
const isFactionCard = computed(
  () => kind.value === "knowledge" && knowledgeSlot.value === "wv_faction",
);
const isReligionCard = computed(
  () => kind.value === "knowledge" && knowledgeSlot.value === "wv_religion",
);
const isMajorEventCard = computed(
  () => kind.value === "knowledge" && knowledgeSlot.value === "wv_major_event",
);
const worldviewVisual = computed(() =>
  kind.value === "knowledge" ? worldviewFanVisual(knowledgeSlot.value) : null,
);
const knowledgeDisplayLabel = computed(() => {
  const slot = knowledgeSlot.value;
  const wvKey = worldviewFanTitleKey(slot);
  if (wvKey) return t(wvKey);
  const srKey = storyRulesFanTitleKey(slot);
  if (srKey) return t(srKey);
  if (isStoryRulesSlot(slot)) return t(STORY_RULES_SLOT.titleKey);
  return n.value?.label ?? "";
});
const wordTarget = computed(() =>
  t("workspace.wordTarget", {
    min: n.value?.word_count_min ?? 0,
    max: n.value?.word_count_max ?? 0,
  }),
);
const chapterTarget = computed(() =>
  t("workspace.chapterTarget", { n: n.value?.chapter_count ?? 0 }),
);
const wordWritten = computed(() => t("workspace.wordWritten", { n: n.value?.word_count ?? 0 }));
const volumeSummary = computed(() =>
  kind.value === "volume" ? volumeDisplaySummary(n.value?.volume) : "",
);
const volumeCollapsed = computed(() => !!n.value?.volumeCollapsed);
const collapsedChapterCount = computed(() => n.value?.collapsedChapterCount ?? 0);
const toggleVolumeCollapse = inject<(id: string) => void>("novework.toggleVolumeCollapse");

function onToggleVolumeCollapse(ev: MouseEvent) {
  ev.preventDefault();
  ev.stopPropagation();
  const id = n.value?.id;
  if (id) toggleVolumeCollapse?.(id);
}

const plotBadge = computed(() => {
  const s = (n.value?.side_plot?.status || "active").trim().toLowerCase();
  if (s === "resolved" || s === "deferred") return s;
  return "active";
});
const plotAbsorbed = computed(() => !!n.value?.side_plot?.absorbed);

const kindIcon = computed((): { icon: Component; class: string } | null => {
  switch (kind.value) {
    case "chapter":
      return { icon: BookOpen, class: "text-muted-foreground" };
    case "volume":
      return { icon: Layers, class: "text-violet-700" };
    case "character":
      return { icon: User, class: "text-amber-700" };
    case "side_plot":
      return { icon: GitBranch, class: "text-sky-700" };
    case "knowledge":
      if (isRaceCard.value) return { icon: Library, class: "text-rose-700" };
      if (isFactionCard.value) return { icon: Library, class: "text-indigo-700" };
      if (isReligionCard.value) return { icon: Library, class: "text-orange-700" };
      if (isMajorEventCard.value) return { icon: Library, class: "text-amber-800" };
      if (worldviewVisual.value) return { icon: Library, class: worldviewVisual.value.icon };
      return { icon: Library, class: "text-teal-700" };
    default:
      return null;
  }
});

const shellClass = computed(() => {
  switch (kind.value) {
    case "novel":
      return "border-primary/40 bg-[oklch(0.92_0.04_155)]";
    case "volume":
      return "border-violet-700/35 bg-[oklch(0.96_0.03_300)] min-w-[180px]";
    case "character":
      return "border-amber-700/30 bg-[oklch(0.97_0.02_85)] min-w-[160px]";
    case "side_plot":
      return "border-sky-700/30 bg-[oklch(0.95_0.03_220)]";
    case "knowledge":
      if (isRaceCard.value) return "border-rose-700/35 bg-[oklch(0.97_0.03_25)] min-w-[160px]";
      if (isFactionCard.value) return "border-indigo-700/35 bg-[oklch(0.96_0.03_280)] min-w-[160px]";
      if (isReligionCard.value) return "border-orange-700/35 bg-[oklch(0.97_0.04_55)] min-w-[160px]";
      if (isMajorEventCard.value) return "border-amber-800/35 bg-[oklch(0.97_0.04_75)] min-w-[160px]";
      if (worldviewVisual.value) return worldviewVisual.value.shell;
      return "border-teal-700/30 bg-[oklch(0.96_0.03_175)] min-w-[160px]";
    case "chapter":
      return "border-border bg-card min-w-[180px]";
    default:
      return "border-border bg-card";
  }
});

const glowTone = computed(() => {
  if (!props.selected) return "";
  if (kind.value === "character") return "story-node-glow story-node-glow--amber";
  if (kind.value === "side_plot") return "story-node-glow story-node-glow--sky";
  if (kind.value === "knowledge") {
    if (isRaceCard.value) return "story-node-glow story-node-glow--rose";
    if (isFactionCard.value) return "story-node-glow story-node-glow--indigo";
    if (isReligionCard.value) return "story-node-glow story-node-glow--amber";
    if (isMajorEventCard.value) return "story-node-glow story-node-glow--amber";
    if (worldviewVisual.value) return worldviewVisual.value.glow;
    return "story-node-glow story-node-glow--teal";
  }
  if (kind.value === "volume") return "story-node-glow story-node-glow--violet";
  return "story-node-glow story-node-glow--primary";
});

/** 人物/剧情/知识卡左右同色；章节仍左琥珀右天蓝作挂载提示。线类型由两端卡片 kind 决定。 */
const sideHandleClass = computed(() => {
  if (kind.value === "character") return "!h-2.5 !w-2.5 !border-2 !border-amber-600 !bg-amber-500";
  if (kind.value === "side_plot") return "!h-2.5 !w-2.5 !border-2 !border-sky-600 !bg-sky-500";
  if (kind.value === "knowledge") {
    if (isRaceCard.value) return "!h-2.5 !w-2.5 !border-2 !border-rose-600 !bg-rose-500";
    if (isFactionCard.value) return "!h-2.5 !w-2.5 !border-2 !border-indigo-600 !bg-indigo-500";
    if (isReligionCard.value) return "!h-2.5 !w-2.5 !border-2 !border-orange-600 !bg-orange-500";
    if (isMajorEventCard.value) return "!h-2.5 !w-2.5 !border-2 !border-amber-700 !bg-amber-600";
    if (worldviewVisual.value) return worldviewVisual.value.handle;
    return "!h-2.5 !w-2.5 !border-2 !border-teal-700 !bg-teal-600";
  }
  return null;
});
const leftHandleClass = computed(
  () => sideHandleClass.value ?? "!h-2.5 !w-2.5 !border-2 !border-rose-600 !bg-rose-500",
);
const rightHandleClass = computed(
  () => sideHandleClass.value ?? "!h-2.5 !w-2.5 !border-2 !border-sky-600 !bg-sky-500",
);

const characterDisplayName = computed(
  () => n.value?.label?.trim() || t("workspace.newCharacter"),
);
</script>

<template>
  <div
    class="relative rounded-lg border px-3 py-2 text-xs shadow-sm"
    :class="[shellClass, glowTone]"
    :aria-selected="selected"
    :style="kind === 'chapter' || kind === 'volume' ? 'max-width: 220px' : 'max-width: 200px'"
  >
    <Handle
      id="top"
      type="source"
      :position="Position.Top"
      class="!h-2.5 !w-2.5 !border-2 !border-primary !bg-primary/80"
      :title="t('workspace.handlePlotTop')"
    />
    <Handle
      id="bottom"
      type="source"
      :position="Position.Bottom"
      class="!h-2.5 !w-2.5 !border-2 !border-primary !bg-primary/80"
      :title="t('workspace.handlePlotBottom')"
    />
    <Handle
      id="left"
      type="source"
      :position="Position.Left"
      :class="leftHandleClass"
      :title="kind === 'side_plot' ? t('workspace.plotCard') : kind === 'knowledge' ? t('workspace.knowledgeCard') : t('workspace.role')"
    />
    <Handle
      id="right"
      type="source"
      :position="Position.Right"
      :class="rightHandleClass"
      :title="kind === 'character' ? t('workspace.role') : kind === 'knowledge' ? t('workspace.knowledgeCard') : t('workspace.plotCard')"
    />

    <template v-if="kind === 'character'">
      <div class="mb-1 flex min-w-0 items-center gap-1.5 text-sm font-semibold leading-tight">
        <component
          v-if="kindIcon"
          :is="kindIcon.icon"
          class="h-3.5 w-3.5 shrink-0"
          :class="kindIcon.class"
        />
        <span class="min-w-0 flex-1 truncate">{{ characterDisplayName }}</span>
      </div>
      <div v-if="n?.character" class="mb-1 flex flex-wrap gap-1">
        <span
          v-if="n.character.gender"
          class="max-w-full truncate rounded bg-muted px-1.5 py-0.5 text-[10px]"
        >{{ n.character.gender }}</span>
        <span
          v-if="n.character.age"
          class="max-w-full truncate rounded bg-muted px-1.5 py-0.5 text-[10px]"
        >{{ n.character.age }}</span>
        <span
          v-if="n.character.world_position?.social_role || n.character.role"
          class="max-w-full truncate rounded bg-amber-100 px-1 text-[10px] text-amber-900"
        >{{ n.character.world_position?.social_role || n.character.role }}</span>
        <span
          v-if="n.character.world_position?.faction || n.character.alignment"
          class="max-w-full truncate rounded bg-muted px-1 text-[10px]"
        >{{ n.character.world_position?.faction || n.character.alignment }}</span>
      </div>
      <p
        v-if="n?.character?.aliases"
        class="mb-1 line-clamp-1 text-[10px] text-muted-foreground"
      >
        {{ n.character.aliases }}
      </p>
      <p
        v-if="n?.character"
        class="line-clamp-2 text-[11px] leading-snug text-muted-foreground"
      >
        {{
          n.character.core_belief?.belief ||
          n.character.personality ||
          t("node.personalityPending")
        }}
      </p>
    </template>

    <template v-else-if="kind === 'side_plot'">
      <div class="flex items-center gap-1.5 font-medium leading-tight">
        <component
          v-if="kindIcon"
          :is="kindIcon.icon"
          class="h-3.5 w-3.5 shrink-0"
          :class="kindIcon.class"
        />
        <span class="truncate">{{ n?.label }}</span>
      </div>
      <div class="mt-1 flex flex-wrap gap-1">
        <span
          class="rounded px-1 text-[10px]"
          :class="
            plotBadge === 'resolved'
              ? 'bg-emerald-100 text-emerald-900'
              : plotBadge === 'deferred' || plotAbsorbed
                ? 'bg-muted text-muted-foreground'
                : 'bg-sky-100 text-sky-900'
          "
        >
          {{
            plotAbsorbed
              ? t("workspace.plotAbsorbedBadge")
              : plotBadge === "resolved"
                ? t("workspace.plotStatusResolved")
                : plotBadge === "deferred"
                  ? t("workspace.plotStatusDeferred")
                  : t("workspace.plotStatusActive")
          }}
        </span>
      </div>
      <p class="mt-1 line-clamp-2 text-[11px] text-muted-foreground">{{ n?.outline }}</p>
    </template>

    <template v-else-if="kind === 'knowledge'">
      <div class="mb-1 flex items-center gap-1.5 text-sm font-semibold leading-tight">
        <component
          v-if="kindIcon"
          :is="kindIcon.icon"
          class="h-3.5 w-3.5 shrink-0"
          :class="kindIcon.class"
        />
        <span class="truncate">{{ knowledgeDisplayLabel }}</span>
      </div>
      <p class="line-clamp-2 text-[11px] text-muted-foreground">
        {{ n?.knowledge?.extracted || n?.knowledge?.extract_prompt || t("workspace.knowledgePending") }}
      </p>
    </template>

    <template v-else-if="kind === 'novel'">
      <div class="flex gap-2">
        <div
          v-if="coverUrl"
          class="h-16 w-11 shrink-0 overflow-hidden rounded border border-primary/20 bg-muted"
        >
          <img :src="coverUrl" class="h-full w-full object-cover" alt="" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="text-sm font-medium leading-tight">{{ n?.label }}</div>
          <p
            v-if="n?.word_count_min || n?.word_count_max"
            class="mt-1 text-[11px] font-medium text-primary"
          >
            {{ wordTarget }}
          </p>
          <p
            v-if="n?.chapter_count"
            class="mt-0.5 text-[11px] font-medium text-primary"
          >
            {{ chapterTarget }}
          </p>
        </div>
      </div>
      <p v-if="n?.outline" class="mt-1 line-clamp-2 text-[11px] text-muted-foreground">{{ n.outline }}</p>
    </template>

    <template v-else-if="kind === 'volume'">
      <div class="flex items-start gap-1">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-1.5 font-medium leading-tight">
            <component v-if="kindIcon" :is="kindIcon.icon" class="h-3.5 w-3.5 shrink-0" :class="kindIcon.class" />
            <span class="truncate">{{ n?.label }}</span>
          </div>
          <p class="mt-0.5 text-[10px] text-violet-800">{{ t("workspace.volumeBadge") }}</p>
        </div>
        <button
          type="button"
          class="nodrag nopan -mr-1 -mt-0.5 shrink-0 rounded p-0.5 text-violet-800 hover:bg-violet-700/10"
          :title="volumeCollapsed ? t('workspace.volumeExpand') : t('workspace.volumeCollapse')"
          :aria-label="volumeCollapsed ? t('workspace.volumeExpand') : t('workspace.volumeCollapse')"
          :aria-expanded="!volumeCollapsed"
          @click.stop="onToggleVolumeCollapse"
          @pointerdown.stop
          @mousedown.stop
        >
          <component :is="volumeCollapsed ? ChevronRight : ChevronDown" class="h-3.5 w-3.5" />
        </button>
      </div>
      <p v-if="volumeCollapsed && collapsedChapterCount" class="mt-1 text-[10px] text-violet-800/80">
        {{ t("workspace.volumeCollapsedCount", { n: collapsedChapterCount }) }}
      </p>
      <p v-else-if="volumeSummary" class="mt-1 line-clamp-3 text-[11px] leading-snug text-muted-foreground">
        {{ volumeSummary }}
      </p>
      <p v-else class="mt-1 text-[10px] text-muted-foreground/70">{{ t("workspace.noOutline") }}</p>
    </template>

    <!-- 章节卡：标题 / 字数 / 大纲（剧情排序在左侧编辑栏） -->
    <template v-else-if="kind === 'chapter'">
      <div class="flex items-center gap-1.5 font-medium leading-tight">
        <component v-if="kindIcon" :is="kindIcon.icon" class="h-3.5 w-3.5 shrink-0" :class="kindIcon.class" />
        <span class="truncate">{{ n?.label }}</span>
      </div>
      <p v-if="n?.word_count" class="mt-0.5 text-[10px] text-muted-foreground">{{ wordWritten }}</p>
      <p v-if="n?.outline" class="mt-1 line-clamp-3 text-[11px] leading-snug text-muted-foreground">
        {{ n.outline }}
      </p>
      <p v-else class="mt-1 text-[10px] text-muted-foreground/70">{{ t("workspace.noOutline") }}</p>
    </template>

    <template v-else>
      <div class="flex items-center gap-1.5 font-medium leading-tight">
        <component v-if="kindIcon" :is="kindIcon.icon" class="h-3.5 w-3.5 shrink-0" :class="kindIcon.class" />
        <span class="truncate">{{ n?.label }}</span>
      </div>
      <p v-if="n?.word_count" class="mt-0.5 text-[10px] text-muted-foreground">{{ wordWritten }}</p>
      <p v-if="n?.outline" class="mt-1 line-clamp-2 text-[11px] text-muted-foreground">{{ n.outline }}</p>
    </template>
  </div>
</template>

<style scoped>
/* Static selection chrome — no pulse (avoids idle GPU + blink). */
.story-node-glow {
  --glow: 61 107 79;
  z-index: 1;
  border-width: 2px;
  box-shadow:
    0 0 0 2px rgb(var(--glow) / 0.45),
    0 0 12px 2px rgb(var(--glow) / 0.28),
    0 2px 8px rgb(0 0 0 / 0.08);
}
.story-node-glow--primary {
  --glow: 61 107 79;
  border-color: rgb(61 107 79 / 0.7);
}
.story-node-glow--amber {
  --glow: 217 119 6;
  border-color: rgb(217 119 6 / 0.7);
}
.story-node-glow--sky {
  --glow: 2 132 199;
  border-color: rgb(2 132 199 / 0.7);
}
.story-node-glow--teal {
  --glow: 15 118 110;
  border-color: rgb(15 118 110 / 0.7);
}
.story-node-glow--violet {
  --glow: 124 58 237;
  border-color: rgb(124 58 237 / 0.7);
}
.story-node-glow--rose {
  --glow: 225 29 72;
  border-color: rgb(225 29 72 / 0.7);
}
.story-node-glow--indigo {
  --glow: 79 70 229;
  border-color: rgb(79 70 229 / 0.7);
}
.story-node-glow--emerald {
  --glow: 5 150 105;
  border-color: rgb(5 150 105 / 0.7);
}
</style>
