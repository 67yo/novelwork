<script setup lang="ts">
import { computed, type Component } from "vue";
import { Handle, Position, type NodeProps } from "@vue-flow/core";
import { BookOpen, GitBranch, Library, User } from "@lucide/vue";
import type { TreeNode } from "@/lib/api";
import { useI18n } from "@/i18n";

/** 根节点可附带封面 URL（来自 NovelProject，不进树 JSON） */
type StoryNodeData = TreeNode & { cover_url?: string };

const props = defineProps<NodeProps<StoryNodeData>>();
const { t } = useI18n();

const n = computed(() => props.data);
const coverUrl = computed(() => n.value?.cover_url?.trim() || "");
const kind = computed(() => n.value?.kind ?? "chapter");
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

const kindIcon = computed((): { icon: Component; class: string } | null => {
  switch (kind.value) {
    case "chapter":
      return { icon: BookOpen, class: "text-muted-foreground" };
    case "character":
      return { icon: User, class: "text-amber-700" };
    case "side_plot":
      return { icon: GitBranch, class: "text-sky-700" };
    case "knowledge":
      return { icon: Library, class: "text-teal-700" };
    default:
      return null;
  }
});

const shellClass = computed(() => {
  switch (kind.value) {
    case "novel":
      return "border-primary/40 bg-[oklch(0.92_0.04_155)]";
    case "character":
      return "border-amber-700/30 bg-[oklch(0.97_0.02_85)] min-w-[160px]";
    case "side_plot":
      return "border-sky-700/30 bg-[oklch(0.95_0.03_220)]";
    case "knowledge":
      return "border-teal-700/30 bg-[oklch(0.96_0.03_175)] min-w-[160px]";
    default:
      return "border-border bg-card";
  }
});

const glowTone = computed(() => {
  if (!props.selected) return "";
  if (kind.value === "character") return "story-node-glow story-node-glow--amber";
  if (kind.value === "side_plot") return "story-node-glow story-node-glow--sky";
  if (kind.value === "knowledge") return "story-node-glow story-node-glow--teal";
  return "story-node-glow story-node-glow--primary";
});

/** 人物/剧情/知识卡左右同色；章节仍左琥珀右天蓝作挂载提示。线类型由两端卡片 kind 决定。 */
const sideHandleClass = computed(() => {
  if (kind.value === "character") return "!h-2.5 !w-2.5 !border-2 !border-amber-600 !bg-amber-500";
  if (kind.value === "side_plot") return "!h-2.5 !w-2.5 !border-2 !border-sky-600 !bg-sky-500";
  if (kind.value === "knowledge") return "!h-2.5 !w-2.5 !border-2 !border-teal-700 !bg-teal-600";
  return null;
});
const leftHandleClass = computed(
  () => sideHandleClass.value ?? "!h-2.5 !w-2.5 !border-2 !border-amber-600 !bg-amber-500",
);
const rightHandleClass = computed(
  () => sideHandleClass.value ?? "!h-2.5 !w-2.5 !border-2 !border-sky-600 !bg-sky-500",
);
</script>

<template>
  <div
    class="relative rounded-lg border px-3 py-2 text-xs shadow-sm"
    :class="[shellClass, glowTone]"
    :aria-selected="selected"
    style="max-width: 200px"
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

    <template v-if="kind === 'character' && n?.character">
      <div class="mb-1 flex items-center justify-between gap-2">
        <span class="flex min-w-0 items-center gap-1.5 text-sm font-semibold leading-tight">
          <component
            v-if="kindIcon"
            :is="kindIcon.icon"
            class="h-3.5 w-3.5 shrink-0"
            :class="kindIcon.class"
          />
          <span class="truncate">{{ n.label }}</span>
        </span>
        <span class="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px]">{{ n.character.gender || "—" }}</span>
      </div>
      <div class="mb-1 flex flex-wrap gap-1">
        <span class="rounded bg-amber-100 px-1 text-[10px] text-amber-900">{{ n.character.role }}</span>
        <span class="rounded bg-muted px-1 text-[10px]">{{ n.character.alignment }}</span>
      </div>
      <p class="line-clamp-2 text-[11px] leading-snug text-muted-foreground">
        {{ n.character.personality || t("node.personalityPending") }}
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
        <span class="truncate">{{ n?.label }}</span>
      </div>
      <div class="mb-1 text-[10px] text-teal-800">
        {{ t("workspace.knowledgeBooksCount", { n: n?.knowledge?.book_ids?.length ?? 0 }) }}
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
</style>
