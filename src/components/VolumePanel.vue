<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { useDebounceFn } from "@vueuse/core";
import { Loader2, Plus, Sparkles, Trash2 } from "@lucide/vue";
import {
  emptyKeyBeat,
  formatVolumeExtracted,
  normalizeVolume,
  type VolumeData,
  type VolumeLayerBlock,
} from "@/lib/volume";
import { useI18n, type MessageKey } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  nodeId?: string;
  data: VolumeData | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { volume: VolumeData; nodeId?: string }];
}>();

const { t } = useI18n();
const draft = reactive(normalizeVolume(props.data));
let syncingFromProps = false;

watch(
  () => [props.nodeId, props.data] as const,
  () => {
    syncingFromProps = true;
    Object.assign(draft, normalizeVolume(props.data));
    syncingFromProps = false;
  },
);

function commit() {
  emit("save", { volume: normalizeVolume(draft), nodeId: props.nodeId });
}

const commitDebounced = useDebounceFn(() => commit(), 600);

onBeforeUnmount(() => {
  commitDebounced.flush();
});

watch(
  draft,
  () => {
    if (syncingFromProps) return;
    commitDebounced();
  },
  { deep: true },
);

const layers: {
  key: keyof Pick<VolumeData, "layer_setup" | "layer_confrontation" | "layer_resolution">;
  titleKey: MessageKey;
}[] = [
  { key: "layer_setup", titleKey: "workspace.vol.layerSetup" },
  { key: "layer_confrontation", titleKey: "workspace.vol.layerConfrontation" },
  { key: "layer_resolution", titleKey: "workspace.vol.layerResolution" },
];

function layerOf(key: (typeof layers)[number]["key"]): VolumeLayerBlock {
  return draft[key];
}

function addBeat() {
  const next = (draft.key_beats.reduce((m, b) => Math.max(m, b.order), 0) || 0) + 1;
  draft.key_beats.push(emptyKeyBeat(next));
}

function removeBeat(i: number) {
  if (draft.key_beats.length <= 1) {
    draft.key_beats[0] = emptyKeyBeat(1);
  } else {
    draft.key_beats.splice(i, 1);
  }
}

type AiTarget =
  | { kind: "positioning" }
  | { kind: "conflict"; field: "conflict_external" | "conflict_internal" | "conflict_deep" }
  | { kind: "layer"; key: (typeof layers)[number]["key"]; field: "chapters" | "description" }
  | { kind: "beat"; index: number; field: "cost" | "description" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "positioning") return t("workspace.vol.positioning");
  if (p.kind === "conflict") {
    if (p.field === "conflict_external") return t("workspace.vol.conflictExternal");
    if (p.field === "conflict_internal") return t("workspace.vol.conflictInternal");
    return t("workspace.vol.conflictDeep");
  }
  if (p.kind === "layer") {
    const title = t(layers.find((l) => l.key === p.key)!.titleKey);
    return p.field === "chapters"
      ? `${title} · ${t("workspace.vol.chapters")}`
      : `${title} · ${t("workspace.vol.description")}`;
  }
  return p.field === "cost"
    ? `${t("workspace.vol.keyBeat")} · ${t("workspace.vol.cost")}`
    : `${t("workspace.vol.keyBeat")} · ${t("workspace.vol.description")}`;
});

function currentForTarget(target: AiTarget): string {
  if (target.kind === "positioning") return draft.positioning;
  if (target.kind === "conflict") return draft[target.field];
  if (target.kind === "layer") return layerOf(target.key)[target.field];
  return draft.key_beats[target.index]?.[target.field] ?? "";
}

function openAi(target: AiTarget) {
  if (aiBusy.value) return;
  pendingAi.value = target;
  aiPrompt.value = "";
  aiError.value = "";
}

function cancelAi() {
  if (aiBusy.value) return;
  pendingAi.value = null;
  aiPrompt.value = "";
  aiError.value = "";
}

async function confirmAi() {
  const target = pendingAi.value;
  const note = aiPrompt.value.trim();
  if (!target || !note || aiBusy.value) return;
  aiBusy.value = true;
  aiError.value = "";
  try {
    const out = await api.rewriteTextField(
      props.novelId,
      aiFieldLabel.value,
      currentForTarget(target),
      note,
      formatVolumeExtracted(normalizeVolume(draft)),
      undefined,
      props.nodeId,
    );
    const text = out.trim();
    if (target.kind === "positioning") draft.positioning = text;
    else if (target.kind === "conflict") draft[target.field] = text;
    else if (target.kind === "layer") layerOf(target.key)[target.field] = text;
    else if (draft.key_beats[target.index]) draft.key_beats[target.index][target.field] = text;
    pendingAi.value = null;
    aiPrompt.value = "";
    commit();
  } catch (e) {
    aiError.value = String(e);
  } finally {
    aiBusy.value = false;
  }
}

const fieldClass = "text-sm !min-h-8 field-sizing-content resize-y py-1.5";

const injectionPreview = computed(() =>
  formatVolumeExtracted(normalizeVolume(draft as VolumeData)),
);
</script>

<template>
  <div class="space-y-4 text-sm">
    <!-- 本卷定位 -->
    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-sm font-semibold">{{ t("workspace.vol.positioning") }}</label>
        <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'positioning' })">
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.vol.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft.positioning"
        rows="1"
        :class="fieldClass"
        :placeholder="t('workspace.vol.positioningPh')"
        :disabled="busy"
      />
    </div>

    <!-- 三层结构 -->
    <div>
      <p class="mb-2 text-sm font-semibold">{{ t("workspace.vol.layers") }}</p>
      <div class="space-y-2">
        <div v-for="L in layers" :key="L.key" class="space-y-2 rounded-md border bg-violet-50/40 p-3 dark:bg-violet-950/20">
          <p class="text-sm font-semibold">{{ t(L.titleKey) }}</p>
          <div class="space-y-1.5">
            <div class="flex items-center justify-between gap-2">
              <label class="text-[11px] text-muted-foreground">{{ t("workspace.vol.chapters") }}</label>
              <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'layer', key: L.key, field: 'chapters' })">
                <Sparkles class="mr-1 h-3.5 w-3.5" />
                {{ t("workspace.vol.aiRewrite") }}
              </Button>
            </div>
            <Textarea
              v-model="layerOf(L.key).chapters"
              rows="1"
              :class="fieldClass"
              :placeholder="t('workspace.vol.chaptersPh')"
              :disabled="busy"
            />
          </div>
          <div class="space-y-1.5">
            <div class="flex items-center justify-between gap-2">
              <label class="text-[11px] text-muted-foreground">{{ t("workspace.vol.description") }}</label>
              <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'layer', key: L.key, field: 'description' })">
                <Sparkles class="mr-1 h-3.5 w-3.5" />
                {{ t("workspace.vol.aiRewrite") }}
              </Button>
            </div>
            <Textarea
              v-model="layerOf(L.key).description"
              rows="2"
              :class="fieldClass"
              :placeholder="t('workspace.vol.descriptionPh')"
              :disabled="busy"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- 冲突层级 -->
    <div>
      <p class="mb-2 text-sm font-semibold">{{ t("workspace.vol.conflicts") }}</p>
      <div class="space-y-2">
        <div
          v-for="item in [
            { field: 'conflict_external' as const, label: 'workspace.vol.conflictExternal' as const, ph: 'workspace.vol.conflictExternalPh' as const },
            { field: 'conflict_internal' as const, label: 'workspace.vol.conflictInternal' as const, ph: 'workspace.vol.conflictInternalPh' as const },
            { field: 'conflict_deep' as const, label: 'workspace.vol.conflictDeep' as const, ph: 'workspace.vol.conflictDeepPh' as const },
          ]"
          :key="item.field"
          class="space-y-1.5 rounded-md border bg-muted/20 p-3"
        >
          <div class="flex items-center justify-between gap-2">
            <label class="text-sm font-semibold">{{ t(item.label) }}</label>
            <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'conflict', field: item.field })">
              <Sparkles class="mr-1 h-3.5 w-3.5" />
              {{ t("workspace.vol.aiRewrite") }}
            </Button>
          </div>
          <Textarea
            v-model="draft[item.field]"
            rows="1"
            :class="fieldClass"
            :placeholder="t(item.ph)"
            :disabled="busy"
          />
        </div>
      </div>
    </div>

    <!-- 关键节点 -->
    <div>
      <div class="mb-2 flex items-center justify-between gap-2">
        <p class="text-sm font-semibold">{{ t("workspace.vol.keyBeats") }}</p>
        <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="addBeat">
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.vol.addBeat") }}
        </Button>
      </div>
      <div class="space-y-2">
        <div
          v-for="(beat, i) in draft.key_beats"
          :key="i"
          class="space-y-2 rounded-md border bg-sky-50/40 p-3 dark:bg-sky-950/20"
        >
          <div class="flex items-center gap-2">
            <label class="shrink-0 text-[11px] text-muted-foreground">{{ t("workspace.vol.order") }}</label>
            <input
              v-model.number="beat.order"
              type="number"
              min="0"
              class="h-7 w-16 rounded-md border bg-background px-2 text-xs"
              :disabled="busy"
            />
            <Button type="button" size="icon" variant="ghost" class="ml-auto h-7 w-7" :disabled="busy" @click="removeBeat(i)">
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </div>
          <div class="space-y-1.5">
            <div class="flex items-center justify-between gap-2">
              <label class="text-[11px] text-muted-foreground">{{ t("workspace.vol.cost") }}</label>
              <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'beat', index: i, field: 'cost' })">
                <Sparkles class="mr-1 h-3.5 w-3.5" />
                {{ t("workspace.vol.aiRewrite") }}
              </Button>
            </div>
            <Textarea v-model="beat.cost" rows="1" :class="fieldClass" :placeholder="t('workspace.vol.costPh')" :disabled="busy" />
          </div>
          <div class="space-y-1.5">
            <div class="flex items-center justify-between gap-2">
              <label class="text-[11px] text-muted-foreground">{{ t("workspace.vol.description") }}</label>
              <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi({ kind: 'beat', index: i, field: 'description' })">
                <Sparkles class="mr-1 h-3.5 w-3.5" />
                {{ t("workspace.vol.aiRewrite") }}
              </Button>
            </div>
            <Textarea v-model="beat.description" rows="2" :class="fieldClass" :placeholder="t('workspace.vol.beatDescPh')" :disabled="busy" />
          </div>
        </div>
      </div>
    </div>

    <!-- 写作注入预览（只读，不落 outline） -->
    <div class="shrink-0 space-y-1.5 border-t pt-3">
      <label class="block text-sm font-semibold">{{ t("workspace.volumeOutline") }}</label>
      <Textarea
        :model-value="injectionPreview"
        rows="6"
        readonly
        class="text-sm resize-y min-h-[8rem] bg-muted/30"
        :placeholder="t('workspace.volumeOutlinePh')"
      />
      <p class="text-[11px] text-muted-foreground">{{ t("workspace.volumeOutlineHint") }}</p>
    </div>

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-4 shadow-lg">
        <p class="mb-2 text-sm font-medium">{{ aiFieldLabel }} · {{ t("workspace.vol.aiRewrite") }}</p>
        <Textarea v-model="aiPrompt" rows="4" class="text-sm" :placeholder="t('workspace.vol.aiPromptPh')" />
        <p v-if="aiError" class="mt-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="mt-3 flex justify-end gap-2">
          <Button variant="ghost" size="sm" :disabled="aiBusy" @click="cancelAi">{{ t("character.aiCancel") }}</Button>
          <Button size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.vol.aiConfirm") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
