<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Sparkles } from "@lucide/vue";
import type { InfoFlowData } from "@/lib/infoFlow";
import { formatInfoFlowExtracted, normalizeInfoFlow } from "@/lib/infoFlow";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

const props = defineProps<{
  novelId: string;
  data: InfoFlowData | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { infoFlow: InfoFlowData; extracted: string }];
}>();

const { t } = useI18n();

const draft = reactive(normalizeInfoFlow(props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeInfoFlow(props.data));
  },
);

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

const fieldBlocks = computed(() => [
  {
    kind: "premise" as const,
    label: t("workspace.if.premise"),
    ph: t("workspace.if.premisePh"),
    model: "premise" as const,
  },
  {
    kind: "info_speed" as const,
    label: t("workspace.if.infoSpeed"),
    ph: t("workspace.if.infoSpeedPh"),
    model: "info_speed" as const,
  },
  {
    kind: "info_barrier" as const,
    label: t("workspace.if.infoBarrier"),
    ph: t("workspace.if.infoBarrierPh"),
    model: "info_barrier" as const,
  },
  {
    kind: "rumor_truth" as const,
    label: t("workspace.if.rumorTruth"),
    ph: t("workspace.if.rumorTruthPh"),
    model: "rumor_truth" as const,
  },
  {
    kind: "knowledge_carrier" as const,
    label: t("workspace.if.knowledgeCarrier"),
    ph: t("workspace.if.knowledgeCarrierPh"),
    model: "knowledge_carrier" as const,
  },
]);

function commit() {
  const infoFlow = normalizeInfoFlow(draft);
  emit("save", {
    infoFlow,
    extracted: formatInfoFlowExtracted(infoFlow),
  });
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "info_speed" }
  | { kind: "info_barrier" }
  | { kind: "rumor_truth" }
  | { kind: "knowledge_carrier" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  const block = fieldBlocks.value.find((b) => b.kind === p.kind);
  return block?.label ?? "";
});

function currentForTarget(p: AiTarget): string {
  if (p.kind === "premise") return draft.premise;
  if (p.kind === "info_speed") return draft.info_speed;
  if (p.kind === "info_barrier") return draft.info_barrier;
  if (p.kind === "rumor_truth") return draft.rumor_truth;
  return draft.knowledge_carrier;
}

function applyAiResult(p: AiTarget, text: string) {
  if (p.kind === "premise") draft.premise = text;
  else if (p.kind === "info_speed") draft.info_speed = text;
  else if (p.kind === "info_barrier") draft.info_barrier = text;
  else if (p.kind === "rumor_truth") draft.rumor_truth = text;
  else draft.knowledge_carrier = text;
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
      formatInfoFlowExtracted(normalizeInfoFlow(draft)),
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
    <WorldviewFanTitle slot="wv_info_flow" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.if.hint") }}</p>

    <div
      v-for="block in fieldBlocks"
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
        v-model="draft[block.model]"
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
