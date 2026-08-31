<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles, Trash2 } from "@lucide/vue";
import type { CoreLawsData, WorldAxiom } from "@/lib/coreLaws";
import { formatCoreLawsExtracted, normalizeCoreLaws } from "@/lib/coreLaws";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";
import WorldviewFanTitle from "@/components/WorldviewFanTitle.vue";

export type AxiomLinkItem = { id: string; title: string };

const props = defineProps<{
  novelId: string;
  data: CoreLawsData | null | undefined;
  /** 挂在核心法则上的公理子卡标题 */
  axiomLinks: AxiomLinkItem[];
  /** 用于写入 extracted 的公理内容（来自子卡） */
  linkedAxioms: WorldAxiom[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { coreLaws: CoreLawsData; extracted: string }];
  addAxiom: [];
  openAxiom: [id: string];
}>();

const { t } = useI18n();

const draft = reactive(normalizeCoreLaws(props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, normalizeCoreLaws(props.data));
  },
);

function commit() {
  const coreLaws = normalizeCoreLaws(draft);
  coreLaws.axioms = [];
  emit("save", {
    coreLaws,
    extracted: formatCoreLawsExtracted(coreLaws, props.linkedAxioms),
  });
}

function addTaboo() {
  draft.taboos.push("");
  commit();
}

function removeTaboo(i: number) {
  if (draft.taboos.length <= 1) {
    draft.taboos[0] = "";
  } else {
    draft.taboos.splice(i, 1);
  }
  commit();
}

type AiTarget =
  | { kind: "premise" }
  | { kind: "power_system" }
  | { kind: "power_expression" }
  | { kind: "taboo"; index: number };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p.kind === "premise") return t("workspace.coreLaws.premise");
  if (p.kind === "power_system") return t("workspace.coreLaws.powerSystem");
  if (p.kind === "power_expression") return t("workspace.coreLaws.powerExpression");
  return t("workspace.coreLaws.taboo");
});

function currentForTarget(p: AiTarget): string {
  if (p.kind === "premise") return draft.premise;
  if (p.kind === "power_system") return draft.power_system;
  if (p.kind === "power_expression") return draft.power_expression;
  return draft.taboos[p.index] ?? "";
}

function applyAiResult(p: AiTarget, text: string) {
  if (p.kind === "premise") draft.premise = text;
  else if (p.kind === "power_system") draft.power_system = text;
  else if (p.kind === "power_expression") draft.power_expression = text;
  else {
    while (draft.taboos.length <= p.index) draft.taboos.push("");
    draft.taboos[p.index] = text;
  }
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
      formatCoreLawsExtracted(normalizeCoreLaws(draft), props.linkedAxioms),
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
    <WorldviewFanTitle slot="wv_core_laws" />
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.coreLaws.hint") }}</p>

    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.coreLaws.premise") }}</label>
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
        class="text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5"
        :placeholder="t('workspace.coreLaws.premisePh')"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <!-- 世界公理：仅标题列表，内容在子卡 -->
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.coreLaws.axioms") }}</label>
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 px-2 text-xs"
          :disabled="busy"
          @click="emit('addAxiom')"
        >
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.addAxiom") }}
        </Button>
      </div>
      <p v-if="!axiomLinks.length" class="text-[11px] text-muted-foreground">
        {{ t("workspace.coreLaws.axiomsEmpty") }}
      </p>
      <button
        v-for="(ax, i) in axiomLinks"
        :key="ax.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border px-2.5 py-1.5 text-left text-sm hover:bg-muted/50"
        @click="emit('openAxiom', ax.id)"
      >
        <span class="shrink-0 text-[11px] text-muted-foreground">{{
          t("workspace.coreLaws.axiomN", { n: i + 1 })
        }}</span>
        <span class="min-w-0 flex-1 truncate font-medium">{{
          ax.title.trim() || t("workspace.coreLaws.axiomUntitled")
        }}</span>
      </button>
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.coreLaws.taboos") }}</label>
        <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="addTaboo">
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.addTaboo") }}
        </Button>
      </div>
      <div v-for="(_, i) in draft.taboos" :key="i" class="flex items-start gap-2">
        <Textarea
          v-model="draft.taboos[i]"
          rows="1"
          class="min-w-0 flex-1 text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5"
          :placeholder="t('workspace.coreLaws.tabooPh')"
          :disabled="busy"
          @change="commit"
        />
        <div class="flex shrink-0 flex-col gap-1">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            class="h-7 px-1.5"
            :disabled="busy || aiBusy"
            :title="t('workspace.coreLaws.aiRewrite')"
            @click="openAi({ kind: 'taboo', index: i })"
          >
            <Sparkles class="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            class="h-7 px-1.5 text-destructive"
            :disabled="busy"
            @click="removeTaboo(i)"
          >
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </div>

    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.coreLaws.powerSystem") }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: 'power_system' })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft.power_system"
        rows="1"
        class="text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5"
        :placeholder="t('workspace.coreLaws.powerSystemPh')"
        :disabled="busy"
        @change="commit"
      />
    </div>

    <div class="space-y-1.5 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="text-xs font-medium text-muted-foreground">{{ t("workspace.coreLaws.powerExpression") }}</label>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          class="h-7 px-2 text-xs"
          :disabled="busy || aiBusy"
          @click="openAi({ kind: 'power_expression' })"
        >
          <Sparkles class="mr-1 h-3.5 w-3.5" />
          {{ t("workspace.coreLaws.aiRewrite") }}
        </Button>
      </div>
      <Textarea
        v-model="draft.power_expression"
        rows="1"
        class="text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5"
        :placeholder="t('workspace.coreLaws.powerExpressionPh')"
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
        <p
          v-if="currentForTarget(pendingAi).trim()"
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
