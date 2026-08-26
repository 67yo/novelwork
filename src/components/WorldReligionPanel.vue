<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Sparkles } from "@lucide/vue";
import type { WorldReligion } from "@/lib/historyCulture";
import {
  emptyReligion,
  formatWorldReligionExtracted,
  normalizeReligion,
  parseWorldReligionJson,
} from "@/lib/historyCulture";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  name: string;
  data: WorldReligion | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { name: string; worldReligion: WorldReligion; extracted: string }];
}>();

const { t } = useI18n();
const draftName = ref(props.name);
const draft = reactive(normalizeReligion(props.data ?? emptyReligion()));

watch(
  () => [props.name, props.data] as const,
  () => {
    draftName.value = props.name;
    Object.assign(draft, normalizeReligion(props.data ?? emptyReligion()));
  },
);

function commit() {
  const worldReligion = normalizeReligion(draft);
  const title =
    worldReligion.name.trim() ||
    draftName.value.trim() ||
    t("workspace.hc.religionUntitled");
  worldReligion.name = worldReligion.name.trim() || title;
  emit("save", {
    name: title,
    worldReligion,
    extracted: formatWorldReligionExtracted(worldReligion),
  });
}

type AiField = keyof WorldReligion | "whole";
const pendingField = ref<AiField | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const f = pendingField.value;
  if (!f) return "";
  if (f === "whole") return t("workspace.hc.aiRewriteWholeReligion");
  if (f === "name") return t("workspace.hc.religionName");
  if (f === "core_belief") return t("workspace.hc.coreBelief");
  return t("workspace.hc.followersScope");
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
    const ctx = formatWorldReligionExtracted(normalizeReligion(draft));
    if (field === "whole") {
      const instruction = `${note}

请只输出一个 JSON 对象，字段：name、core_belief、followers_scope（可用中文键 名称/核心信念/追随者范围）。不要解释、不要 markdown 围栏。`;
      const out = await api.rewriteTextField(props.novelId, aiFieldLabel.value, ctx, instruction, ctx);
      const parsed = parseWorldReligionJson(out);
      if (!parsed) {
        aiError.value = t("workspace.hc.religionParseFail");
        return;
      }
      Object.assign(draft, parsed);
      draftName.value = parsed.name.trim() || draftName.value;
    } else {
      const out = await api.rewriteTextField(props.novelId, aiFieldLabel.value, draft[field], note, ctx);
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
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.hc.religionCardHint") }}</p>
    <div class="space-y-1">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs text-muted-foreground">{{ t("workspace.hc.religionName") }}</label>
        <Button type="button" size="sm" variant="ghost" class="h-7 px-2 text-xs" :disabled="busy || aiBusy" @click="openAi('name')">
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Input v-model="draft.name" class="h-8" :placeholder="t('workspace.hc.religionNamePh')" :disabled="busy" @change="commit" />
    </div>
    <div v-for="field in (['core_belief', 'followers_scope'] as const)" :key="field" class="space-y-1">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs text-muted-foreground">{{
          field === "core_belief" ? t("workspace.hc.coreBelief") : t("workspace.hc.followersScope")
        }}</label>
        <Button type="button" size="sm" variant="ghost" class="h-6 px-1.5 text-[11px]" :disabled="busy || aiBusy" @click="openAi(field)">
          <Sparkles class="mr-0.5 h-3 w-3" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea v-model="draft[field]" rows="1" class="text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5" :disabled="busy" @change="commit" />
    </div>
    <Button type="button" size="sm" variant="outline" class="h-7 w-fit px-2 text-xs" :disabled="busy || aiBusy" @click="openAi('whole')">
      <Sparkles class="mr-1 h-3.5 w-3.5" />
      {{ t("workspace.hc.aiRewriteWholeReligion") }}
    </Button>
    <div v-if="pendingField" class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4" @click.self="cancelAi">
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.coreLaws.aiTitle") }}</h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ aiFieldLabel }}</p>
        <Textarea v-model="aiPrompt" rows="4" class="mt-3 text-sm" :disabled="aiBusy" :placeholder="t('workspace.coreLaws.aiPromptPh')" />
        <p v-if="aiError" class="mt-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <Button size="sm" variant="outline" :disabled="aiBusy" @click="cancelAi">{{ t("novels.cancel") }}</Button>
          <Button size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1.5 h-3.5 w-3.5 animate-spin" />
            {{ aiBusy ? t("workspace.coreLaws.aiBusy") : t("workspace.coreLaws.aiRun") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
