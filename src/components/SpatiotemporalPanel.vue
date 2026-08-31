<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles } from "@lucide/vue";
import type { KeyLocation, SpatiotemporalData } from "@/lib/spatiotemporal";
import { formatSpatiotemporalExtracted, normalizeSpatiotemporal, parseKeyLocationsJson } from "@/lib/spatiotemporal";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

export type LocationLinkItem = { id: string; title: string };

const props = defineProps<{
  novelId: string;
  data: SpatiotemporalData | null | undefined;
  locationLinks: LocationLinkItem[];
  linkedLocations: KeyLocation[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { spatiotemporal: SpatiotemporalData; extracted: string }];
  addLocation: [];
  openLocation: [id: string];
  addLocations: [locations: KeyLocation[]];
}>();

const { t } = useI18n();

const draft = reactive(normalizeSpatiotemporal(props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeSpatiotemporal(props.data));
  },
);

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

function commit() {
  const spatiotemporal = normalizeSpatiotemporal(draft);
  spatiotemporal.locations = [];
  emit("save", {
    spatiotemporal,
    extracted: formatSpatiotemporalExtracted(spatiotemporal, props.linkedLocations),
  });
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "era" }
  | { kind: "ecology" }
  | { kind: "world_pattern" }
  | { kind: "atmosphere" }
  | { kind: "locations_batch" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "premise") return t("workspace.st.premise");
  if (p.kind === "era") return t("workspace.st.era");
  if (p.kind === "ecology") return t("workspace.st.ecology");
  if (p.kind === "world_pattern") return t("workspace.st.worldPattern");
  if (p.kind === "atmosphere") return t("workspace.st.atmosphere");
  return t("workspace.st.locations");
});

function currentForTarget(p: AiTarget): string {
  if (p.kind === "premise") return draft.premise;
  if (p.kind === "era") return draft.era;
  if (p.kind === "ecology") return draft.ecology;
  if (p.kind === "world_pattern") return draft.world_pattern;
  if (p.kind === "atmosphere") return draft.atmosphere;
  return formatSpatiotemporalExtracted(
    normalizeSpatiotemporal(draft),
    props.linkedLocations,
  );
}

function applyAiResult(p: AiTarget, text: string) {
  if (p.kind === "premise") draft.premise = text;
  else if (p.kind === "era") draft.era = text;
  else if (p.kind === "ecology") draft.ecology = text;
  else if (p.kind === "world_pattern") draft.world_pattern = text;
  else if (p.kind === "atmosphere") draft.atmosphere = text;
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
    if (target.kind === "locations_batch") {
      const instruction = `${note}

请只输出 JSON 数组，每项含 name、features、terrain、faction 四个字符串字段（可用中文键 名称/特征/地貌/控制势力）。不要解释、不要 markdown 围栏。`;
      const out = await api.rewriteTextField(
        props.novelId,
        aiFieldLabel.value,
        currentForTarget(target),
        instruction,
        formatSpatiotemporalExtracted(
          normalizeSpatiotemporal(draft),
          props.linkedLocations,
        ),
      );
      const locs = parseKeyLocationsJson(out);
      if (!locs.length) {
        aiError.value = t("workspace.st.locationsParseFail");
        return;
      }
      emit("addLocations", locs);
    } else {
      const out = await api.rewriteTextField(
        props.novelId,
        aiFieldLabel.value,
        currentForTarget(target),
        note,
        formatSpatiotemporalExtracted(
          normalizeSpatiotemporal(draft),
          props.linkedLocations,
        ),
      );
      applyAiResult(target, out.trim());
    }
    pendingAi.value = null;
    aiPrompt.value = "";
    if (target.kind !== "locations_batch") commit();
  } catch (e) {
    aiError.value = String(e);
  } finally {
    aiBusy.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <WorldviewFanTitle slot="wv_spatiotemporal" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.st.hint") }}</p>

    <div
      v-for="block in (
        [
          { kind: 'premise' as const, label: t('workspace.st.premise'), ph: t('workspace.st.premisePh'), model: 'premise' },
          { kind: 'era' as const, label: t('workspace.st.era'), ph: t('workspace.st.eraPh'), model: 'era' },
          { kind: 'ecology' as const, label: t('workspace.st.ecology'), ph: t('workspace.st.ecologyPh'), model: 'ecology' },
          { kind: 'world_pattern' as const, label: t('workspace.st.worldPattern'), ph: t('workspace.st.worldPatternPh'), model: 'world_pattern' },
        ]
      )"
      :key="block.kind"
      class="space-y-1.5 rounded-md border bg-muted/20 p-3"
    >
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ block.label }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: block.kind })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="(draft as unknown as Record<string, string>)[block.model]"
        rows="1"
        :class="fieldClass"
        :placeholder="block.ph"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <!-- 关键地点：仅标题列表，内容在子卡 -->
    <div class="space-y-2">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.st.locations") }}</label>
        <div class="flex flex-wrap gap-1.5">
          <Button
            type="button"
            size="sm"
            variant="outline"
            class="h-7 px-2 text-xs"
            :disabled="busy || aiBusy"
            @click="openAi({ kind: 'locations_batch' })"
          >
            <Sparkles class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.st.genLocations") }}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            class="h-7 px-2 text-xs"
            :disabled="busy"
            @click="emit('addLocation')"
          >
            <Plus class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.st.addLocation") }}
          </Button>
        </div>
      </div>
      <p v-if="!locationLinks.length" class="text-[11px] text-muted-foreground">
        {{ t("workspace.st.locationsEmpty") }}
      </p>
      <button
        v-for="(loc, i) in locationLinks"
        :key="loc.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm hover:bg-muted/50"
        @click="emit('openLocation', loc.id)"
      >
        <span class="shrink-0 text-[11px] text-muted-foreground">{{
          t("workspace.st.locationN", { n: i + 1 })
        }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{
          loc.title.trim() || t("workspace.st.locationUntitled")
        }}</span>
      </button>
    </div>

    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.st.atmosphere") }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: 'atmosphere' })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft.atmosphere"
        rows="1"
        :class="fieldClass"
        :placeholder="t('workspace.st.atmospherePh')"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">
          {{
            pendingAi.kind === "locations_batch"
              ? t("workspace.st.genLocationsTitle")
              : t("workspace.coreLaws.aiTitle")
          }}
        </h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ aiFieldLabel }}</p>
        <p
          v-if="pendingAi.kind !== 'locations_batch' && currentForTarget(pendingAi).trim()"
          class="mt-2 max-h-24 overflow-y-auto whitespace-pre-wrap rounded border bg-muted/40 px-2 py-1.5 text-xs text-muted-foreground"
        >
          {{ currentForTarget(pendingAi) }}
        </p>
        <label class="mt-3 block text-xs text-muted-foreground">{{ t("workspace.coreLaws.aiPrompt") }}</label>
        <Textarea
          v-model="aiPrompt"
          rows="4"
          class="mt-1 text-sm"
          :disabled="aiBusy"
          :placeholder="
            pendingAi.kind === 'locations_batch'
              ? t('workspace.st.genLocationsPh')
              : t('workspace.coreLaws.aiPromptPh')
          "
        />
        <p v-if="aiError" class="mt-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="outline" :disabled="aiBusy" @click="cancelAi">
            {{ t("novels.cancel") }}
          </Button>
          <Button size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            <Sparkles v-else class="mr-1.5 h-3.5 w-3.5" />
            {{
              aiBusy
                ? t("workspace.coreLaws.aiBusy")
                : pendingAi.kind === "locations_batch"
                  ? t("workspace.st.genLocationsRun")
                  : t("workspace.coreLaws.aiRun")
            }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
