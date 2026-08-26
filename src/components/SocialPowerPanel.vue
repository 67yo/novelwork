<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles } from "@lucide/vue";
import type { MajorFaction, SocialPowerData, WorldRace } from "@/lib/socialPower";
import { formatSocialPowerExtracted, normalizeSocialPower } from "@/lib/socialPower";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

export type ExtLinkItem = { id: string; title: string };

const props = defineProps<{
  novelId: string;
  data: SocialPowerData | null | undefined;
  raceLinks: ExtLinkItem[];
  factionLinks: ExtLinkItem[];
  linkedRaces: WorldRace[];
  linkedFactions: MajorFaction[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { socialPower: SocialPowerData; extracted: string }];
  addRace: [];
  addFaction: [];
  openRace: [id: string];
  openFaction: [id: string];
}>();

const { t } = useI18n();
const draft = reactive(normalizeSocialPower(props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeSocialPower(props.data));
  },
);

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

function commit() {
  const socialPower = normalizeSocialPower(draft);
  socialPower.races = [];
  socialPower.factions = [];
  emit("save", {
    socialPower,
    extracted: formatSocialPowerExtracted(
      socialPower,
      props.linkedRaces,
      props.linkedFactions,
    ),
  });
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "class_structure" }
  | { kind: "political_system" }
  | { kind: "power_visibility" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "premise") return t("workspace.pow.premise");
  if (p.kind === "class_structure") return t("workspace.pow.classStructure");
  if (p.kind === "political_system") return t("workspace.pow.politicalSystem");
  return t("workspace.pow.powerVisibility");
});

function currentForTarget(p: AiTarget): string {
  if (p.kind === "premise") return draft.premise;
  if (p.kind === "class_structure") return draft.class_structure;
  if (p.kind === "political_system") return draft.political_system;
  return draft.power_visibility;
}

function applyAiResult(p: AiTarget, text: string) {
  if (p.kind === "premise") draft.premise = text;
  else if (p.kind === "class_structure") draft.class_structure = text;
  else if (p.kind === "political_system") draft.political_system = text;
  else draft.power_visibility = text;
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
    const ctx = formatSocialPowerExtracted(
      normalizeSocialPower(draft),
      props.linkedRaces,
      props.linkedFactions,
    );
    const out = await api.rewriteTextField(
      props.novelId,
      aiFieldLabel.value,
      currentForTarget(target),
      note,
      ctx,
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
    <WorldviewFanTitle slot="wv_social_power" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.pow.hint") }}</p>

    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.pow.premise") }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: 'premise' })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft.premise"
        rows="1"
        :class="fieldClass"
        :placeholder="t('workspace.pow.premisePh')"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <!-- 种族 -->
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.pow.races") }}</label>
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 px-2 text-xs"
          :disabled="busy"
          @click="emit('addRace')"
        >
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.pow.addRace") }}
        </Button>
      </div>
      <p v-if="!raceLinks.length" class="text-[11px] text-muted-foreground">
        {{ t("workspace.pow.racesEmpty") }}
      </p>
      <button
        v-for="(item, i) in raceLinks"
        :key="item.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border border-rose-200/80 bg-rose-50/50 px-2.5 py-1.5 text-left text-sm hover:bg-rose-50 dark:border-rose-900/40 dark:bg-rose-950/20"
        @click="emit('openRace', item.id)"
      >
        <span class="shrink-0 text-[11px] text-rose-700/80 dark:text-rose-300/80">{{
          t("workspace.pow.raceN", { n: i + 1 })
        }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{
          item.title.trim() || t("workspace.pow.raceUntitled")
        }}</span>
      </button>
    </div>

    <!-- 主要势力 -->
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.pow.factions") }}</label>
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 px-2 text-xs"
          :disabled="busy"
          @click="emit('addFaction')"
        >
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.pow.addFaction") }}
        </Button>
      </div>
      <p v-if="!factionLinks.length" class="text-[11px] text-muted-foreground">
        {{ t("workspace.pow.factionsEmpty") }}
      </p>
      <button
        v-for="(item, i) in factionLinks"
        :key="item.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border border-indigo-200/80 bg-indigo-50/50 px-2.5 py-1.5 text-left text-sm hover:bg-indigo-50 dark:border-indigo-900/40 dark:bg-indigo-950/20"
        @click="emit('openFaction', item.id)"
      >
        <span class="shrink-0 text-[11px] text-indigo-700/80 dark:text-indigo-300/80">{{
          t("workspace.pow.factionN", { n: i + 1 })
        }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{
          item.title.trim() || t("workspace.pow.factionUntitled")
        }}</span>
      </button>
    </div>

    <div
      v-for="block in (
        [
          { kind: 'class_structure' as const, label: t('workspace.pow.classStructure'), ph: t('workspace.pow.classStructurePh'), model: 'class_structure' },
          { kind: 'political_system' as const, label: t('workspace.pow.politicalSystem'), ph: t('workspace.pow.politicalSystemPh'), model: 'political_system' },
          { kind: 'power_visibility' as const, label: t('workspace.pow.powerVisibility'), ph: t('workspace.pow.powerVisibilityPh'), model: 'power_visibility' },
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

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("workspace.coreLaws.aiTitle") }}</h2>
        <p class="mt-1 text-xs text-muted-foreground">{{ aiFieldLabel }}</p>
        <label class="mt-3 block text-xs text-muted-foreground">{{ t("workspace.coreLaws.aiPrompt") }}</label>
        <Textarea
          v-model="aiPrompt"
          rows="4"
          class="mt-1 text-sm"
          :disabled="aiBusy"
          :placeholder="t('workspace.coreLaws.aiPromptPh')"
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
