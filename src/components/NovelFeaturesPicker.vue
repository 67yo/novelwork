<script setup lang="ts">
import { reactive, ref, watch } from "vue";
import { ChevronDown, ChevronUp } from "@lucide/vue";
import { useI18n, type MessageKey } from "@/i18n";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  FEATURE_GROUPS,
  addCustomFeature,
  customValues,
  emptyNovelFeatures,
  normalizeNovelFeatures,
  toggleFeatureValue,
  type FeatureGroupDef,
  type FeatureGroupKey,
  type NovelFeatures,
} from "@/lib/novelFeatures";

const props = withDefaults(
  defineProps<{
    modelValue?: NovelFeatures | null;
    disabled?: boolean;
  }>(),
  { modelValue: null, disabled: false },
);

const emit = defineEmits<{
  "update:modelValue": [NovelFeatures];
}>();

const { t } = useI18n();

function groupLabel(key: FeatureGroupKey): string {
  return t(`novels.feat.group.${key}` as MessageKey);
}

function optLabel(id: string): string {
  return t(`novels.feat.opt.${id}` as MessageKey);
}

const local = reactive<NovelFeatures>(emptyNovelFeatures());
const customDraft = reactive<Record<FeatureGroupKey, string>>({
  genres: "",
  core_play: "",
  styles: "",
  relationships: "",
  audiences: "",
});
const customOpen = ref<Partial<Record<FeatureGroupKey, boolean>>>({});
/** 有已选项时默认折叠未选项；点展开后显示全部 */
const groupExpanded = ref<Partial<Record<FeatureGroupKey, boolean>>>({});

watch(
  () => props.modelValue,
  (v) => {
    const n = normalizeNovelFeatures(v);
    for (const g of FEATURE_GROUPS) {
      local[g.key] = [...n[g.key]];
      if (customValues(n, g.key).length) customOpen.value[g.key] = true;
    }
  },
  { immediate: true, deep: true },
);

function emitLocal() {
  emit("update:modelValue", {
    genres: [...local.genres],
    core_play: [...local.core_play],
    styles: [...local.styles],
    relationships: [...local.relationships],
    audiences: [...local.audiences],
  });
}

function hasAnySelected(group: FeatureGroupKey): boolean {
  return local[group].length > 0;
}

/** 无选择 → 全开；有选择且未展开 → 仅已选 */
function isCollapsed(group: FeatureGroupKey): boolean {
  return hasAnySelected(group) && !groupExpanded.value[group];
}

function visiblePresetIds(g: FeatureGroupDef): readonly string[] {
  if (!isCollapsed(g.key)) return g.options;
  return g.options.filter((id) => local[g.key].includes(id));
}

function showCustomChip(g: FeatureGroupDef): boolean {
  if (!g.allowCustom) return false;
  if (!isCollapsed(g.key)) return true;
  return !!customOpen.value[g.key] || customValues(local, g.key).length > 0;
}

function hiddenCount(g: FeatureGroupDef): number {
  if (!isCollapsed(g.key)) return 0;
  let n = g.options.filter((id) => !local[g.key].includes(id)).length;
  if (g.allowCustom && !customOpen.value[g.key] && customValues(local, g.key).length === 0) {
    n += 1; // 自定义入口也算折叠项
  }
  return n;
}

function toggleExpand(group: FeatureGroupKey) {
  groupExpanded.value[group] = !groupExpanded.value[group];
}

function toggle(group: FeatureGroupKey, id: string) {
  if (props.disabled) return;
  Object.assign(local, toggleFeatureValue(local, group, id));
  // 取消到空：自动恢复全量展示
  if (local[group].length === 0) groupExpanded.value[group] = false;
  emitLocal();
}

function openCustom(group: FeatureGroupKey) {
  if (props.disabled) return;
  customOpen.value[group] = !customOpen.value[group];
}

function submitCustom(group: FeatureGroupKey) {
  if (props.disabled) return;
  Object.assign(local, addCustomFeature(local, group, customDraft[group]));
  customDraft[group] = "";
  customOpen.value[group] = true;
  emitLocal();
}

function removeCustom(group: FeatureGroupKey, value: string) {
  if (props.disabled) return;
  local[group] = local[group].filter((x) => x !== value);
  if (local[group].length === 0) groupExpanded.value[group] = false;
  emitLocal();
}

function selected(group: FeatureGroupKey, id: string) {
  return local[group].includes(id);
}
</script>

<template>
  <div class="space-y-2">
    <div
      v-for="g in FEATURE_GROUPS"
      :key="g.key"
      class="space-y-1"
    >
      <div class="flex flex-wrap items-center gap-1.5">
        <span class="shrink-0 text-xs font-medium text-muted-foreground">
          {{ groupLabel(g.key) }}
        </span>
        <button
          v-for="id in visiblePresetIds(g)"
          :key="id"
          type="button"
          class="rounded-md border px-2 py-1 text-xs transition"
          :class="
            selected(g.key, id)
              ? 'border-primary bg-primary/10 text-foreground'
              : 'border-border bg-background text-muted-foreground hover:bg-muted/60'
          "
          :disabled="disabled"
          @click="toggle(g.key, id)"
        >
          {{ optLabel(id) }}
        </button>
        <button
          v-if="showCustomChip(g)"
          type="button"
          class="rounded-md border px-2 py-1 text-xs transition"
          :class="
            customOpen[g.key] || customValues(local, g.key).length
              ? 'border-primary bg-primary/10 text-foreground'
              : 'border-border bg-background text-muted-foreground hover:bg-muted/60'
          "
          :disabled="disabled"
          @click="openCustom(g.key)"
        >
          {{ t("novels.feat.custom") }}
        </button>
        <button
          v-for="c in customValues(local, g.key)"
          :key="c"
          type="button"
          class="rounded-md border border-primary/40 bg-primary/10 px-2 py-1 text-xs"
          :disabled="disabled"
          :title="t('novels.feat.removeCustom')"
          @click="removeCustom(g.key, c)"
        >
          {{ c }} ×
        </button>
        <button
          v-if="hasAnySelected(g.key)"
          type="button"
          class="inline-flex h-7 w-7 items-center justify-center rounded-md border border-border text-muted-foreground hover:bg-muted/60"
          :aria-expanded="!!groupExpanded[g.key]"
          :aria-label="
            groupExpanded[g.key] ? t('novels.feat.collapse') : t('novels.feat.expand')
          "
          :title="
            groupExpanded[g.key]
              ? t('novels.feat.collapse')
              : t('novels.feat.expandMore', { n: hiddenCount(g) })
          "
          :disabled="disabled"
          @click="toggleExpand(g.key)"
        >
          <ChevronUp v-if="groupExpanded[g.key]" class="h-3.5 w-3.5" />
          <ChevronDown v-else class="h-3.5 w-3.5" />
        </button>
      </div>
      <div
        v-if="g.allowCustom && customOpen[g.key] && !isCollapsed(g.key)"
        class="flex items-center gap-1.5"
      >
        <Input
          v-model="customDraft[g.key]"
          class="h-7 text-xs"
          :placeholder="t('novels.feat.customPh')"
          :disabled="disabled"
          @keydown.enter.prevent="submitCustom(g.key)"
        />
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 shrink-0 px-2 text-xs"
          :disabled="disabled || !customDraft[g.key].trim()"
          @click="submitCustom(g.key)"
        >
          {{ t("novels.feat.customAdd") }}
        </Button>
      </div>
    </div>
  </div>
</template>
