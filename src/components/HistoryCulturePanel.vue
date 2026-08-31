<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles } from "@lucide/vue";
import type { HistoryCultureData, MajorEvent, WorldReligion } from "@/lib/historyCulture";
import { formatHistoryCultureExtracted, normalizeHistoryCulture } from "@/lib/historyCulture";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

export type ChildLinkItem = { id: string; title: string };

const props = defineProps<{
  novelId: string;
  data: HistoryCultureData | null | undefined;
  religionLinks: ChildLinkItem[];
  eventLinks: ChildLinkItem[];
  linkedReligions: WorldReligion[];
  linkedEvents: MajorEvent[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { historyCulture: HistoryCultureData; extracted: string }];
  addReligion: [];
  addEvent: [];
  openReligion: [id: string];
  openEvent: [id: string];
}>();

const { t } = useI18n();
const draft = reactive(normalizeHistoryCulture(props.data));
const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeHistoryCulture(props.data));
  },
);

function commit() {
  const historyCulture = normalizeHistoryCulture(draft);
  historyCulture.religions = [];
  historyCulture.major_events = [];
  emit("save", {
    historyCulture,
    extracted: formatHistoryCultureExtracted(
      historyCulture,
      props.linkedReligions,
      props.linkedEvents,
    ),
  });
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "customs" }
  | { kind: "economy" }
  | { kind: "daily_slices" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "premise") return t("workspace.hc.premise");
  if (p.kind === "customs") return t("workspace.hc.customs");
  if (p.kind === "economy") return t("workspace.hc.economy");
  return t("workspace.hc.dailySlices");
});

function currentForTarget(p: AiTarget): string {
  return draft[p.kind];
}

function applyAiResult(p: AiTarget, text: string) {
  draft[p.kind] = text;
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
      formatHistoryCultureExtracted(normalizeHistoryCulture(draft), props.linkedReligions, props.linkedEvents),
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
    <WorldviewFanTitle slot="wv_history_culture" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.hc.hint") }}</p>

    <div
      v-for="field in (['premise', 'customs', 'economy', 'daily_slices'] as const)"
      :key="field"
      class="space-y-1.5 rounded-md border bg-muted/20 p-3"
    >
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{
          field === "premise"
            ? t("workspace.hc.premise")
            : field === "customs"
              ? t("workspace.hc.customs")
              : field === "economy"
                ? t("workspace.hc.economy")
                : t("workspace.hc.dailySlices")
        }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: field })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft[field]"
        rows="1"
        :class="fieldClass"
        :placeholder="
          field === 'premise'
            ? t('workspace.hc.premisePh')
            : field === 'customs'
              ? t('workspace.hc.customsPh')
              : field === 'economy'
                ? t('workspace.hc.economyPh')
                : t('workspace.hc.dailySlicesPh')
        "
        :disabled="busy"
        @change="commit"
      />
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.hc.religions") }}</label>
        <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="emit('addReligion')">
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.hc.addReligion") }}
        </Button>
      </div>
      <p v-if="!religionLinks.length" class="text-[11px] text-muted-foreground">{{ t("workspace.hc.religionsEmpty") }}</p>
      <button
        v-for="(item, i) in religionLinks"
        :key="item.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm hover:bg-muted/50"
        @click="emit('openReligion', item.id)"
      >
        <span class="shrink-0 text-[11px] text-muted-foreground">{{ t("workspace.hc.religionN", { n: i + 1 }) }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{ item.title.trim() || t("workspace.hc.religionUntitled") }}</span>
      </button>
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.hc.majorEvents") }}</label>
        <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="emit('addEvent')">
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.hc.addEvent") }}
        </Button>
      </div>
      <p v-if="!eventLinks.length" class="text-[11px] text-muted-foreground">{{ t("workspace.hc.eventsEmpty") }}</p>
      <button
        v-for="(item, i) in eventLinks"
        :key="item.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm hover:bg-muted/50"
        @click="emit('openEvent', item.id)"
      >
        <span class="shrink-0 text-[11px] text-muted-foreground">{{ t("workspace.hc.eventN", { n: i + 1 }) }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{ item.title.trim() || t("workspace.hc.eventUntitled") }}</span>
      </button>
    </div>

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.coreLaws.aiTitle") }}</h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ aiFieldLabel }}</p>
        <label class="mt-3 block text-xs text-muted-foreground">{{ t("workspace.coreLaws.aiPrompt") }}</label>
        <Textarea v-model="aiPrompt" rows="4" class="mt-1 text-sm" :disabled="aiBusy" :placeholder="t('workspace.coreLaws.aiPromptPh')" />
        <p v-if="aiError" class="mt-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="outline" :disabled="aiBusy" @click="cancelAi">{{ t("novels.cancel") }}</Button>
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
