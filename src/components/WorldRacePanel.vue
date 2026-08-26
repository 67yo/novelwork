<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Sparkles } from "@lucide/vue";
import type { WorldRace } from "@/lib/socialPower";
import {
  emptyRace,
  formatWorldRaceExtracted,
  normalizeRace,
  parseWorldRaceJson,
} from "@/lib/socialPower";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  name: string;
  data: WorldRace | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { name: string; worldRace: WorldRace; extracted: string }];
}>();

const { t } = useI18n();
const draftName = ref(props.name);
const draft = reactive(normalizeRace(props.data ?? emptyRace()));

watch(
  () => [props.name, props.data] as const,
  () => {
    draftName.value = props.name;
    Object.assign(draft, normalizeRace(props.data ?? emptyRace()));
  },
);

function commit() {
  const worldRace = normalizeRace(draft);
  const title =
    worldRace.name.trim() ||
    draftName.value.trim() ||
    t("workspace.pow.raceUntitled");
  worldRace.name = worldRace.name.trim() || title;
  emit("save", {
    name: title,
    worldRace,
    extracted: formatWorldRaceExtracted(worldRace),
  });
}

type AiField = keyof WorldRace | "whole";
const pendingField = ref<AiField | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const f = pendingField.value;
  if (!f) return "";
  if (f === "whole") return t("workspace.pow.aiRewriteWholeRace");
  if (f === "name") return t("workspace.pow.raceName");
  if (f === "features") return t("workspace.pow.raceFeatures");
  if (f === "population") return t("workspace.pow.racePopulation");
  return t("workspace.pow.raceSocialStatus");
});

function openAi(field: AiField) {
  if (aiBusy.value) return;
  pendingField.value = field;
  aiPrompt.value = "";
  aiError.value = "";
}

function cancelAi() {
  if (aiBusy.value) return;
  pendingField.value = null;
  aiPrompt.value = "";
  aiError.value = "";
}

async function confirmAi() {
  const field = pendingField.value;
  const note = aiPrompt.value.trim();
  if (!field || !note || aiBusy.value) return;
  aiBusy.value = true;
  aiError.value = "";
  try {
    const ctx = formatWorldRaceExtracted(normalizeRace(draft));
    if (field === "whole") {
      const instruction = `${note}

请只输出一个 JSON 对象，字段：name、features、population、social_status（可用中文键 名称/特征/人口/社会地位）。不要解释、不要 markdown 围栏。`;
      const out = await api.rewriteTextField(
        props.novelId,
        aiFieldLabel.value,
        ctx,
        instruction,
        ctx,
      );
      const parsed = parseWorldRaceJson(out);
      if (!parsed) {
        aiError.value = t("workspace.pow.raceParseFail");
        return;
      }
      Object.assign(draft, parsed);
      draftName.value = parsed.name.trim() || draftName.value;
    } else {
      const out = await api.rewriteTextField(
        props.novelId,
        aiFieldLabel.value,
        draft[field],
        note,
        ctx,
      );
      draft[field] = out.trim();
      if (field === "name") draftName.value = draft.name;
    }
    pendingField.value = null;
    aiPrompt.value = "";
    commit();
  } catch (e) {
    aiError.value = String(e);
  } finally {
    aiBusy.value = false;
  }
}

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <div class="flex items-start justify-between gap-2">
      <p class="text-[11px] text-muted-foreground">{{ t("workspace.pow.raceCardHint") }}</p>
      <Button
        type="button"
        size="sm"
        variant="outline"
        class="h-7 shrink-0 px-2 text-xs"
        :disabled="busy || aiBusy"
        @click="openAi('whole')"
      >
        <Sparkles class="mr-1 h-3.5 w-3.5" />
        {{ t("workspace.coreLaws.aiRewriteWhole") }}
      </Button>
    </div>
    <div
      v-for="field in (['name', 'features', 'population', 'social_status'] as const)"
      :key="field"
      class="space-y-1"
    >
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs text-muted-foreground">
          {{
            field === "name"
              ? t("workspace.pow.raceName")
              : field === "features"
                ? t("workspace.pow.raceFeatures")
                : field === "population"
                  ? t("workspace.pow.racePopulation")
                  : t("workspace.pow.raceSocialStatus")
          }}
        </label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-6 px-1.5 text-[11px]"
          :disabled="busy || aiBusy"
          @click="openAi(field)"
        >
          <Sparkles class="mr-0.5 h-3 w-3" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Input
        v-if="field === 'name'"
        v-model="draft.name"
        class="h-8"
        :placeholder="t('workspace.pow.raceNamePh')"
        :disabled="busy"
        @change="commit"
      />
      <Textarea
        v-else
        v-model="draft[field]"
        rows="1"
        :class="fieldClass"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <div
      v-if="pendingField"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">
          {{
            pendingField === "whole"
              ? t("workspace.pow.aiWholeTitleRace")
              : t("workspace.coreLaws.aiTitle")
          }}
        </h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ aiFieldLabel }}</p>
        <label class="mt-3 block text-xs text-muted-foreground">{{ t("workspace.coreLaws.aiPrompt") }}</label>
        <Textarea
          v-model="aiPrompt"
          rows="4"
          class="mt-1 text-sm"
          :disabled="aiBusy"
          :placeholder="
            pendingField === 'whole'
              ? t('workspace.pow.aiWholePromptPhRace')
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
            {{ aiBusy ? t("workspace.coreLaws.aiBusy") : t("workspace.coreLaws.aiRun") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
