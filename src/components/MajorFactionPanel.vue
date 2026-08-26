<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Sparkles } from "@lucide/vue";
import type { MajorFaction } from "@/lib/socialPower";
import {
  emptyFaction,
  formatMajorFactionExtracted,
  normalizeFaction,
  parseMajorFactionJson,
} from "@/lib/socialPower";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  name: string;
  data: MajorFaction | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { name: string; majorFaction: MajorFaction; extracted: string }];
}>();

const { t } = useI18n();
const draftName = ref(props.name);
const draft = reactive(normalizeFaction(props.data ?? emptyFaction()));

watch(
  () => [props.name, props.data] as const,
  () => {
    draftName.value = props.name;
    Object.assign(draft, normalizeFaction(props.data ?? emptyFaction()));
  },
);

function commit() {
  const majorFaction = normalizeFaction(draft);
  const title =
    majorFaction.name.trim() ||
    majorFaction.faction_type.trim() ||
    draftName.value.trim() ||
    t("workspace.pow.factionUntitled");
  if (!majorFaction.name.trim()) majorFaction.name = title;
  emit("save", {
    name: title,
    majorFaction,
    extracted: formatMajorFactionExtracted(majorFaction),
  });
}

type AiField = keyof MajorFaction | "whole";
const pendingField = ref<AiField | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const f = pendingField.value;
  if (!f) return "";
  if (f === "whole") return t("workspace.pow.aiRewriteWholeFaction");
  if (f === "name") return t("workspace.pow.factionName");
  if (f === "faction_type") return t("workspace.pow.factionType");
  if (f === "goal") return t("workspace.pow.factionGoal");
  if (f === "means") return t("workspace.pow.factionMeans");
  return t("workspace.pow.factionPowerBase");
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
    const ctx = formatMajorFactionExtracted(normalizeFaction(draft));
    if (field === "whole") {
      const instruction = `${note}

请只输出一个 JSON 对象，字段：name、faction_type、goal、means、power_base（可用中文键 名称/类型/目标/手段/权力基础）。不要解释、不要 markdown 围栏。`;
      const out = await api.rewriteTextField(
        props.novelId,
        aiFieldLabel.value,
        ctx,
        instruction,
        ctx,
      );
      const parsed = parseMajorFactionJson(out);
      if (!parsed) {
        aiError.value = t("workspace.pow.factionParseFail");
        return;
      }
      Object.assign(draft, parsed);
      draftName.value =
        parsed.name.trim() || parsed.faction_type.trim() || draftName.value;
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
      <p class="text-[11px] text-muted-foreground">{{ t("workspace.pow.factionCardHint") }}</p>
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
      v-for="field in (['name', 'faction_type', 'goal', 'means', 'power_base'] as const)"
      :key="field"
      class="space-y-1"
    >
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs text-muted-foreground">
          {{
            field === "name"
              ? t("workspace.pow.factionName")
              : field === "faction_type"
                ? t("workspace.pow.factionType")
                : field === "goal"
                  ? t("workspace.pow.factionGoal")
                  : field === "means"
                    ? t("workspace.pow.factionMeans")
                    : t("workspace.pow.factionPowerBase")
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
        v-if="field === 'name' || field === 'faction_type'"
        v-model="draft[field]"
        class="h-8"
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
              ? t("workspace.pow.aiWholeTitleFaction")
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
              ? t('workspace.pow.aiWholePromptPhFaction')
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
