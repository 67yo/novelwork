<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles, Trash2 } from "@lucide/vue";
import {
  blockDataFromKnowledge,
  buildStoryRulesBlockWholeInstruction,
  formatStoryRulesBlockExtracted,
  listMissingStoryRulesFields,
  normalizeStoryRulesBlock,
  parseStoryRulesBlockJson,
  STORY_RULES_BLOCK_FIELDS,
  storyRulesBlockWholeLabel,
  storyRulesFanTitleKey,
  type StoryRulesBlockSlot,
  type StoryRulesFieldDef,
} from "@/lib/storyRules";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  nodeId?: string;
  slot: StoryRulesBlockSlot;
  /** 知识卡 payload（含 surface_setting / story_engine 等嵌套字段） */
  data: {
    surface_setting?: Record<string, unknown> | null;
    story_engine?: Record<string, unknown> | null;
    fulfillment_system?: Record<string, unknown> | null;
    constraint_redlines?: Record<string, unknown> | null;
  } | null | undefined;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: Record<string, unknown>];
}>();

const { t } = useI18n();

const draft = reactive(blockDataFromKnowledge(props.slot, props.data));

watch(
  () => props.data,
  () => {
    Object.assign(draft, blockDataFromKnowledge(props.slot, props.data));
  },
);

const fieldDefs = computed(() => STORY_RULES_BLOCK_FIELDS[props.slot]);
const titleKey = computed(() => storyRulesFanTitleKey(props.slot)!);
const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-48 resize-y py-1.5";

function commit() {
  const data = normalizeStoryRulesBlock(props.slot, draft as Record<string, unknown>);
  emit("save", data as Record<string, unknown>);
}

function getFieldValue(def: StoryRulesFieldDef): string | string[] {
  const v = (draft as Record<string, unknown>)[def.key];
  if (def.kind === "list") return Array.isArray(v) ? v.map(String) : [""];
  return String(v ?? "");
}

function setTextField(key: string, value: string) {
  (draft as Record<string, unknown>)[key] = value;
}

function listOf(key: string): string[] {
  const v = (draft as Record<string, unknown>)[key];
  return Array.isArray(v) ? v.map(String) : [""];
}

function addListItem(key: string) {
  (draft as Record<string, unknown>)[key] = [...listOf(key), ""];
  commit();
}

function removeListItem(key: string, index: number) {
  const list = [...listOf(key)];
  if (list.length <= 1) list[0] = "";
  else list.splice(index, 1);
  (draft as Record<string, unknown>)[key] = list;
  commit();
}

type AiTarget = { kind: "field"; key: string; index?: number } | { kind: "whole" };

const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");

function currentExtracted(): string {
  return formatStoryRulesBlockExtracted(
    props.slot,
    normalizeStoryRulesBlock(props.slot, draft as Record<string, unknown>),
  );
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

function applyFieldAi(key: string, text: string, index?: number) {
  const def = fieldDefs.value.find((f) => f.key === key);
  if (!def) return;
  if (def.kind === "list" && index != null) {
    const list = [...(getFieldValue(def) as string[])];
    while (list.length <= index) list.push("");
    list[index] = text;
    (draft as Record<string, unknown>)[key] = list;
  } else {
    setTextField(key, text);
  }
}

async function confirmAi() {
  const target = pendingAi.value;
  const note = aiPrompt.value.trim();
  if (!target || !note || aiBusy.value) return;
  aiBusy.value = true;
  aiError.value = "";
  try {
    const ctx = currentExtracted();
    if (target.kind === "whole") {
      const instruction = buildStoryRulesBlockWholeInstruction(
        props.slot,
        note,
        t(titleKey.value),
      );
      let out = await api.rewriteTextField(
        props.novelId,
        storyRulesBlockWholeLabel(props.slot),
        ctx,
        instruction,
        ctx,
        undefined,
        props.nodeId,
      );
      let parsed = parseStoryRulesBlockJson(props.slot, out);
      if (!parsed) {
        const missing = listMissingStoryRulesFields(
          props.slot,
          normalizeStoryRulesBlock(props.slot, parseLooseJson(out) ?? {}),
        );
        if (missing.length) {
          const retry = `${instruction}\n\n【补全】缺少：${missing.join("、")}`;
          out = await api.rewriteTextField(
            props.novelId,
            storyRulesBlockWholeLabel(props.slot),
            ctx,
            retry,
            ctx,
            undefined,
            props.nodeId,
          );
          parsed = parseStoryRulesBlockJson(props.slot, out);
        }
      }
      if (!parsed) {
        aiError.value = t("workspace.sr.parseFail");
        return;
      }
      Object.assign(draft, parsed);
    } else {
      const def = fieldDefs.value.find((f) => f.key === target.key);
      if (!def) return;
      const cur =
        def.kind === "list" && target.index != null
          ? (getFieldValue(def) as string[])[target.index] ?? ""
          : String(getFieldValue(def) ?? "");
      const out = await api.rewriteTextField(
        props.novelId,
        t(def.labelKey),
        cur,
        note,
        ctx,
        undefined,
        props.nodeId,
      );
      applyFieldAi(target.key, out.trim(), target.index);
    }
    pendingAi.value = null;
    aiPrompt.value = "";
    commit();
  } catch (e) {
    aiError.value = String(e);
  } finally {
    aiBusy.value = false;
  }
}

function parseLooseJson(text: string): Record<string, unknown> | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    return JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function showSection(def: StoryRulesFieldDef, i: number): boolean {
  if (!def.sectionKey) return false;
  const prev = fieldDefs.value[i - 1];
  return !prev?.sectionKey || prev.sectionKey !== def.sectionKey;
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <div class="flex items-center justify-between gap-2">
      <h3 class="text-sm font-semibold">{{ t(titleKey) }}</h3>
      <Button
        type="button"
        size="sm"
        variant="outline"
        class="h-7 px-2 text-xs"
        :disabled="busy || aiBusy"
        @click="openAi({ kind: 'whole' })"
      >
        <Sparkles class="mr-1 h-3.5 w-3.5" />
        {{ t("workspace.sr.aiWholeCard") }}
      </Button>
    </div>
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.sr.blockHint") }}</p>

    <template v-for="(def, i) in fieldDefs" :key="def.key">
      <p
        v-if="showSection(def, i)"
        class="text-xs font-medium text-muted-foreground"
      >
        {{ t(def.sectionKey!) }}
      </p>
      <div v-if="def.kind === 'text'" class="space-y-1.5 rounded-md border bg-muted/20 p-3">
        <div class="flex items-center justify-between gap-2">
          <label class="text-xs font-medium text-muted-foreground">{{ t(def.labelKey) }}</label>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            class="h-7 px-2 text-xs"
            :disabled="busy || aiBusy"
            @click="openAi({ kind: 'field', key: def.key })"
          >
            <Sparkles class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.sr.aiRewrite") }}
          </Button>
        </div>
        <Textarea
          :model-value="String(getFieldValue(def))"
          rows="1"
          :class="fieldClass"
          :placeholder="t(def.phKey)"
          :disabled="busy"
          @update:model-value="(v) => { setTextField(def.key, String(v)); }"
          @change="commit"
        />
      </div>
      <div v-else class="space-y-2 rounded-md border bg-muted/20 p-3">
        <div class="flex items-center justify-between gap-2">
          <label class="text-xs font-medium text-muted-foreground">{{ t(def.labelKey) }}</label>
          <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="addListItem(def.key)">
            <Plus class="mr-1 h-3.5 w-3.5" />
            {{ t("workspace.sr.addItem") }}
          </Button>
        </div>
        <div v-for="(_, idx) in getFieldValue(def) as string[]" :key="idx" class="flex gap-2">
          <Textarea
            :model-value="(getFieldValue(def) as string[])[idx]"
            rows="1"
            :class="fieldClass + ' flex-1'"
            :placeholder="t(def.phKey)"
            :disabled="busy"
            @update:model-value="(v) => { applyFieldAi(def.key, String(v), idx); }"
            @change="commit"
          />
          <Button
            type="button"
            size="icon"
            variant="ghost"
            class="h-8 w-8 shrink-0"
            :disabled="busy"
            @click="removeListItem(def.key, idx)"
          >
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            class="h-8 w-8 shrink-0"
            :disabled="busy || aiBusy"
            @click="openAi({ kind: 'field', key: def.key, index: idx })"
          >
            <Sparkles class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </template>

    <div
      v-if="pendingAi"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-4 shadow-lg">
        <p class="mb-2 text-sm font-medium">
          {{ pendingAi.kind === "whole" ? t("workspace.sr.aiWholeCard") : t("workspace.sr.aiRewrite") }}
        </p>
        <Textarea v-model="aiPrompt" rows="4" class="text-sm" :placeholder="t('workspace.sr.aiPromptPh')" />
        <p v-if="aiError" class="mt-2 text-xs text-destructive">{{ aiError }}</p>
        <div class="mt-3 flex justify-end gap-2">
          <Button variant="ghost" size="sm" :disabled="aiBusy" @click="cancelAi">{{ t("character.aiCancel") }}</Button>
          <Button size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
            {{ t("workspace.sr.aiConfirm") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
