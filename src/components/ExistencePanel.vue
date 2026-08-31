<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Sparkles } from "@lucide/vue";
import type { ExistenceData } from "@/lib/existence";
import { formatExistenceExtracted, normalizeExistence } from "@/lib/existence";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

const props = defineProps<{
  novelId: string;
  data: ExistenceData | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { existence: ExistenceData; extracted: string }];
}>();

const { t } = useI18n();

const draft = reactive(normalizeExistence(props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeExistence(props.data));
  },
);

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

function commit() {
  const existence = normalizeExistence(draft);
  emit("save", {
    existence,
    extracted: formatExistenceExtracted(existence),
  });
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "death" }
  | { kind: "calendar" }
  | { kind: "lifespan" }
  | { kind: "disease_reproduction" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "premise") return t("workspace.ex.premise");
  if (p.kind === "death") return t("workspace.ex.death");
  if (p.kind === "calendar") return t("workspace.ex.calendar");
  if (p.kind === "lifespan") return t("workspace.ex.lifespan");
  return t("workspace.ex.diseaseReproduction");
});

function currentForTarget(p: AiTarget): string {
  if (p.kind === "premise") return draft.premise;
  if (p.kind === "death") return draft.death;
  if (p.kind === "calendar") return draft.calendar;
  if (p.kind === "lifespan") return draft.lifespan;
  return draft.disease_reproduction;
}

function applyAiResult(p: AiTarget, text: string) {
  if (p.kind === "premise") draft.premise = text;
  else if (p.kind === "death") draft.death = text;
  else if (p.kind === "calendar") draft.calendar = text;
  else if (p.kind === "lifespan") draft.lifespan = text;
  else draft.disease_reproduction = text;
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
      formatExistenceExtracted(normalizeExistence(draft)),
    );
    applyAiResult(target, out.trim());
    pendingAi.value = null;
    aiPrompt.value = "";
    commit();
  } catch (e) {
    aiError.value = String(e);
  } finally {
    aiBusy.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <WorldviewFanTitle slot="wv_existence" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.ex.hint") }}</p>

    <div
      v-for="block in (
        [
          { kind: 'premise' as const, label: t('workspace.ex.premise'), ph: t('workspace.ex.premisePh'), model: 'premise' },
          { kind: 'death' as const, label: t('workspace.ex.death'), ph: t('workspace.ex.deathPh'), model: 'death' },
          { kind: 'calendar' as const, label: t('workspace.ex.calendar'), ph: t('workspace.ex.calendarPh'), model: 'calendar' },
          { kind: 'lifespan' as const, label: t('workspace.ex.lifespan'), ph: t('workspace.ex.lifespanPh'), model: 'lifespan' },
          { kind: 'disease_reproduction' as const, label: t('workspace.ex.diseaseReproduction'), ph: t('workspace.ex.diseaseReproductionPh'), model: 'disease_reproduction' },
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
        v-model="(draft as Record<string, string>)[block.model]"
        rows="1"
        :class="fieldClass"
        :placeholder="block.ph"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-4 shadow-lg">
        <h3 class="mb-2 text-sm font-medium">{{ t("workspace.coreLaws.aiTitle") }} — {{ aiFieldLabel }}</h3>
        <Textarea
          v-model="aiPrompt"
          rows="3"
          class="mb-2 text-sm"
          :placeholder="t('workspace.coreLaws.aiPromptPh')"
          :disabled="aiBusy"
        />
        <p v-if="aiError" class="mb-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="flex justify-end gap-2">
          <Button type="button" variant="outline" size="sm" :disabled="aiBusy" @click="cancelAi">
            {{ t("novels.cancel") }}
          </Button>
          <Button type="button" size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
            {{ aiBusy ? t("workspace.coreLaws.aiBusy") : t("workspace.coreLaws.aiRun") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
