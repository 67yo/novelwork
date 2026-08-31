<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { Loader2, Plus, Sparkles, Trash2 } from "@lucide/vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { CharacterCard } from "@/lib/characterCard";
import {
  CHARACTER_WHOLE_AI_FIELD_LABEL,
  buildCharacterSheetPrompt,
  buildCharacterWholeAiInstruction,
  emptyRelation,
  formatCharacterExtracted,
  listMissingCharacterFields,
  normalizeCharacterCard,
  parseCharacterCardJson,
  syncLegacyFields,
} from "@/lib/characterCard";
import type { MessageKey } from "@/i18n/messages";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api } from "@/lib/api";

const props = defineProps<{
  novelId: string;
  characterId?: string;
  name: string;
  card: CharacterCard;
  busy?: boolean;
  /** 核心法则公理标题，供「体现法则 / 受塑于」选择 */
  lawOptions?: string[];
  /** 其他人物名，供关系网 / 该知但未知道 */
  characterNames?: string[];
}>();

const emit = defineEmits<{
  save: [payload: { name: string; card: CharacterCard }];
}>();

const { t } = useI18n();

const draftName = ref(props.name);
const draft = reactive(normalizeCharacterCard(props.card));

watch(
  () => [props.name, props.card] as const,
  () => {
    draftName.value = props.name;
    Object.assign(draft, normalizeCharacterCard(props.card));
  },
  { deep: true },
);

const fieldClass = "text-sm !min-h-8 field-sizing-content max-h-40 resize-y py-1.5";
const lawOpts = computed(() => props.lawOptions ?? []);
const nameOpts = computed(() =>
  (props.characterNames ?? []).filter((n) => n.trim() && n.trim() !== draftName.value.trim()),
);

function commit() {
  const card = syncLegacyFields(normalizeCharacterCard(draft));
  Object.assign(draft, card);
  emit("save", {
    name: draftName.value.trim() || props.name,
    card,
  });
}

function addRelation() {
  draft.relations.push(emptyRelation());
  commit();
}

function removeRelation(i: number) {
  draft.relations.splice(i, 1);
  commit();
}

function addCatchphrase() {
  draft.voice.catchphrases.push("");
  commit();
}

function removeCatchphrase(i: number) {
  if (draft.voice.catchphrases.length <= 1) {
    draft.voice.catchphrases[0] = "";
  } else {
    draft.voice.catchphrases.splice(i, 1);
  }
  commit();
}

type AiTarget = "whole" | string;
const pendingAi = ref<AiTarget | null>(null);
const aiPrompt = ref("");
const aiBusy = ref(false);
const aiError = ref("");
const sheetPromptBusy = ref(false);
const sheetBusy = ref(false);
const sheetError = ref("");
const sheetUrl = computed(() =>
  draft.sheet.image_path.trim() ? convertFileSrc(draft.sheet.image_path.trim()) : "",
);

async function draftSheetPrompt() {
  if (sheetPromptBusy.value || sheetBusy.value) return;
  sheetError.value = "";
  if (!props.characterId) {
    draft.sheet.prompt = buildCharacterSheetPrompt(draftName.value || props.name, draft);
    commit();
    return;
  }
  sheetPromptBusy.value = true;
  try {
    draft.sheet.prompt = await api.generateCharacterSheetPrompt(props.novelId, props.characterId);
    commit();
  } catch {
    draft.sheet.prompt = buildCharacterSheetPrompt(draftName.value || props.name, draft);
    commit();
  } finally {
    sheetPromptBusy.value = false;
  }
}

async function generateSheet() {
  if (sheetBusy.value || !props.characterId) {
    sheetError.value = t("character.sheetNeedSave");
    return;
  }
  sheetBusy.value = true;
  sheetError.value = "";
  try {
    const r = await api.generateCharacterSheet(
      props.novelId,
      props.characterId,
      draft.sheet.prompt,
    );
    draft.sheet.prompt = r.prompt;
    draft.sheet.image_path = r.image_path;
    commit();
  } catch (e) {
    sheetError.value = String(e);
  } finally {
    sheetBusy.value = false;
  }
}

const aiFieldLabel = computed(() => {
  const p = pendingAi.value;
  if (!p) return "";
  if (p === "whole") return t("character.aiWhole");
  return p;
});

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

function currentFieldText(_label: string): string {
  // 简单：用整卡上下文；单字段改写时把 label 对应的当前值尽量塞进 current
  const md = formatCharacterExtracted(normalizeCharacterCard(draft), draftName.value);
  return md;
}

function missingFieldLabels(ids: string[]): string {
  const key: Record<string, MessageKey> = {
    trueName: "character.trueName",
    gender: "character.gender",
    age: "character.age",
    aliases: "character.aliases",
    birthClass: "character.birthClass",
    faction: "character.faction",
    socialRole: "character.socialRole",
    conflictStance: "character.conflictStance",
    embodiesLaw: "character.embodiesLaw",
    embodiesNote: "character.lawNotePh",
    shapedBy: "character.shapedBy",
    shapedByNote: "character.lawNotePh",
    willChallenge: "character.willChallenge",
    relations: "character.secRelations",
    belief: "character.belief",
    authorVerdict: "character.authorVerdict",
    beliefSource: "character.beliefSource",
    desireSurface: "character.desireSurface",
    desireDeep: "character.desireDeep",
    fearWhat: "character.fearWhat",
    fearSource: "character.fearSource",
    secretContent: "character.secretContent",
    secretWho: "character.secretWho",
    secretShouldKnow: "character.secretShouldKnow",
    secretExposure: "character.secretExposure",
    lineTrigger: "character.lineTrigger",
    lineSource: "character.lineSource",
    lineReaction: "character.lineReaction",
    traumaWound: "character.traumaWound",
    traumaTrigger: "character.traumaTrigger",
    traumaStress: "character.traumaStress",
    traumaImprint: "character.traumaImprint",
    contraPoles: "character.contraPoles",
    contraSource: "character.contraSource",
    contraTrajectory: "character.contraTrajectory",
    arcGrowth: "character.arcGrowth",
    arcFall: "character.arcFall",
    arcChoice: "character.arcChoice",
    voicePos: "character.voicePos",
    cognitiveFilter: "character.cognitiveFilter",
    bodyLanguage: "character.bodyLanguage",
    sentenceLength: "character.sentenceLength",
    pause: "character.pause",
    patterns: "character.patterns",
    catchphrases: "character.catchphrases",
    emAnger: "character.emAnger",
    emTense: "character.emTense",
    emMask: "character.emMask",
    emSad: "character.emSad",
    emHappy: "character.emHappy",
    banned: "character.banned",
  };
  const relPart: Record<string, MessageKey> = {
    Name: "character.relName",
    Relation: "character.relRelation",
    Definition: "character.relDefinition",
    Attitude: "character.relAttitude",
    Tension: "character.relTension",
  };
  return ids
    .map((id) => {
      const m = id.match(/^rel(\d+)(Name|Relation|Definition|Attitude|Tension)$/);
      if (m) {
        const part = relPart[m[2]!];
        return `${t("character.relationN", { n: m[1] })}·${t(part)}`;
      }
      const k = key[id];
      return k ? t(k) : id;
    })
    .join("、");
}

function extractTrueNameFromJson(out: string): string {
  try {
    const s = out.trim();
    const o = JSON.parse(s.slice(s.indexOf("{"), s.lastIndexOf("}") + 1)) as Record<
      string,
      unknown
    >;
    const n = o.true_name ?? o.真名 ?? o.name ?? o.姓名;
    return n == null ? "" : String(n).trim();
  } catch {
    return "";
  }
}

async function rewriteWholeCard(note: string, ctx: string): Promise<string> {
  const instruction = buildCharacterWholeAiInstruction(note, {
    trueName: draftName.value,
    lawOptions: lawOpts.value,
    characterNames: nameOpts.value,
  });
  return api.rewriteTextField(
    props.novelId,
    CHARACTER_WHOLE_AI_FIELD_LABEL,
    ctx,
    instruction,
    ctx,
    undefined,
    props.characterId,
  );
}

async function confirmAi() {
  const target = pendingAi.value;
  const note = aiPrompt.value.trim();
  if (!target || !note || aiBusy.value) return;
  aiBusy.value = true;
  aiError.value = "";
  try {
    const ctx = formatCharacterExtracted(normalizeCharacterCard(draft), draftName.value);
    if (target === "whole") {
      let out = await rewriteWholeCard(note, ctx);
      let parsed = parseCharacterCardJson(out);
      if (!parsed) {
        aiError.value = t("character.parseFail");
        return;
      }
      let trueName = extractTrueNameFromJson(out) || draftName.value;
      let missing = listMissingCharacterFields(parsed, trueName);
      if (missing.length) {
        const retryNote = `${note}\n\n【补全】上次 JSON 仍缺少或未填：${missing.join("、")}。必须补齐全部人物卡字段。`;
        out = await rewriteWholeCard(retryNote, formatCharacterExtracted(parsed, trueName));
        parsed = parseCharacterCardJson(out);
        if (!parsed) {
          aiError.value = t("character.parseFail");
          return;
        }
        trueName = extractTrueNameFromJson(out) || trueName;
        missing = listMissingCharacterFields(parsed, trueName);
      }
      if (missing.length) {
        aiError.value = t("character.parseIncomplete", {
          fields: missingFieldLabels(missing),
        });
        return;
      }
      if (trueName) draftName.value = trueName;
      Object.assign(draft, syncLegacyFields(parsed));
    } else {
      const out = await api.rewriteTextField(
        props.novelId,
        target,
        currentFieldText(target),
        note,
        ctx,
      );
      // 单字段：把结果当作该字段纯文本写入（调用方用 openAi 传入 path）
      applyFieldRewrite(target, out.trim());
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

function applyFieldRewrite(path: string, text: string) {
  const map: Record<string, (v: string) => void> = {
    aliases: (v) => {
      draft.aliases = v;
    },
    gender: (v) => {
      draft.gender = v;
    },
    age: (v) => {
      draft.age = v;
    },
    "wp.birth_class": (v) => {
      draft.world_position.birth_class = v;
    },
    "wp.faction": (v) => {
      draft.world_position.faction = v;
    },
    "wp.social_role": (v) => {
      draft.world_position.social_role = v;
    },
    "wp.conflict_stance": (v) => {
      draft.world_position.conflict_stance = v;
    },
    "wa.embodies_note": (v) => {
      draft.world_anchors.embodies_note = v;
    },
    "wa.shaped_by_note": (v) => {
      draft.world_anchors.shaped_by_note = v;
    },
    "wa.will_challenge": (v) => {
      draft.world_anchors.will_challenge = v;
    },
    "cb.belief": (v) => {
      draft.core_belief.belief = v;
    },
    "cb.source": (v) => {
      draft.core_belief.source = v;
    },
    "voice.positioning": (v) => {
      draft.voice.positioning = v;
    },
    "voice.cognitive_filter": (v) => {
      draft.voice.cognitive_filter = v;
    },
    "voice.banned": (v) => {
      draft.voice.banned = v;
    },
  };
  const fn = map[path];
  if (fn) fn(text);
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto text-sm">
    <div class="flex items-start justify-between gap-2">
      <div>
        <h3 class="text-base font-semibold">{{ t("character.editTitle") }}</h3>
        <p class="mt-0.5 text-xs text-muted-foreground">{{ t("character.subtitle") }}</p>
      </div>
      <Button
        type="button"
        size="sm"
        variant="outline"
        class="h-7 shrink-0 px-2 text-xs"
        :disabled="busy || aiBusy"
        @click="openAi('whole')"
      >
        <Sparkles class="mr-1 h-3.5 w-3.5" />
        {{ t("character.aiWhole") }}
      </Button>
    </div>

    <!-- 基础 -->
    <section class="space-y-2 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secBasic") }}</p>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.trueName") }}</label>
        <Input v-model="draftName" class="h-8" :placeholder="t('character.trueNamePh')" :disabled="busy" @change="commit" />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.aliases") }}</label>
        <Input v-model="draft.aliases" class="h-8" :placeholder="t('character.aliasesPh')" :disabled="busy" @change="commit" />
      </div>
      <div class="grid gap-2 sm:grid-cols-2">
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.gender") }}</label>
          <Input v-model="draft.gender" class="h-8" :placeholder="t('character.genderUnset')" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.age") }}</label>
          <Input v-model="draft.age" class="h-8" :placeholder="t('character.agePh')" :disabled="busy" @change="commit" />
        </div>
      </div>
    </section>

    <!-- 世界位置 -->
    <section class="space-y-2 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secWorldPos") }}</p>
      <div class="grid gap-2 sm:grid-cols-2">
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.birthClass") }}</label>
          <Input v-model="draft.world_position.birth_class" class="h-8" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.faction") }}</label>
          <Input v-model="draft.world_position.faction" class="h-8" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.socialRole") }}</label>
          <Input v-model="draft.world_position.social_role" class="h-8" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.conflictStance") }}</label>
          <Input
            v-model="draft.world_position.conflict_stance"
            class="h-8"
            :placeholder="t('character.conflictStancePh')"
            :disabled="busy"
            @change="commit"
          />
        </div>
      </div>
    </section>

    <!-- 世界锚点 -->
    <section class="space-y-2 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secAnchors") }}</p>
      <div class="space-y-1.5">
        <label class="block text-[11px] text-muted-foreground">{{ t("character.embodiesLaw") }}</label>
        <Input
          v-model="draft.world_anchors.embodies_law"
          class="h-8"
          list="law-opts"
          :placeholder="t('character.lawPick')"
          :disabled="busy"
          @change="commit"
        />
        <Textarea
          v-model="draft.world_anchors.embodies_note"
          :class="fieldClass"
          rows="1"
          :placeholder="t('character.lawNotePh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
      <div class="space-y-1.5">
        <label class="block text-[11px] text-muted-foreground">{{ t("character.shapedBy") }}</label>
        <Input
          v-model="draft.world_anchors.shaped_by_law"
          class="h-8"
          list="law-opts"
          :placeholder="t('character.lawPick')"
          :disabled="busy"
          @change="commit"
        />
        <datalist id="law-opts">
          <option v-for="law in lawOpts" :key="law" :value="law" />
        </datalist>
        <Textarea
          v-model="draft.world_anchors.shaped_by_note"
          :class="fieldClass"
          rows="1"
          :placeholder="t('character.lawNotePh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.willChallenge") }}</label>
        <Textarea
          v-model="draft.world_anchors.will_challenge"
          :class="fieldClass"
          rows="1"
          :disabled="busy"
          @change="commit"
        />
      </div>
    </section>

    <!-- 关系网络 -->
    <section class="space-y-2 rounded-md border bg-muted/20 p-3">
      <div class="flex items-center justify-between gap-2">
        <p class="text-xs font-medium text-muted-foreground">{{ t("character.secRelations") }}</p>
        <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="addRelation">
          <Plus class="mr-1 h-3.5 w-3.5" />
          {{ t("character.addRelation") }}
        </Button>
      </div>
      <p v-if="!draft.relations.length" class="text-[11px] text-muted-foreground">{{ t("character.relationsEmpty") }}</p>
      <div
        v-for="(rel, i) in draft.relations"
        :key="i"
        class="space-y-2 rounded-md border bg-background/60 p-2"
      >
        <div class="flex items-center justify-between gap-2">
          <span class="text-[11px] text-muted-foreground">{{ t("character.relationN", { n: i + 1 }) }}</span>
          <Button type="button" size="sm" variant="ghost" class="h-6 px-1.5" :disabled="busy" @click="removeRelation(i)">
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.relName") }}</label>
          <Input
            v-model="rel.name"
            class="h-8"
            list="char-name-opts"
            :placeholder="t('character.relNamePh')"
            :disabled="busy"
            @change="commit"
          />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.relRelation") }}</label>
          <Input v-model="rel.relation" class="h-8" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.relDefinition") }}</label>
          <Textarea v-model="rel.definition" :class="fieldClass" rows="1" :disabled="busy" @change="commit" />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.relAttitude") }}</label>
          <Input
            v-model="rel.default_attitude"
            class="h-8"
            :placeholder="t('character.relAttitudePh')"
            :disabled="busy"
            @change="commit"
          />
        </div>
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.relTension") }}</label>
          <Textarea
            v-model="rel.hidden_tension"
            :class="fieldClass"
            rows="1"
            :placeholder="t('character.relTensionPh')"
            :disabled="busy"
            @change="commit"
          />
        </div>
      </div>
      <datalist id="char-name-opts">
        <option v-for="n in nameOpts" :key="n" :value="n" />
      </datalist>
    </section>

    <!-- 核心信念 -->
    <section class="space-y-2 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secBelief") }}</p>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.belief") }}</label>
        <Textarea v-model="draft.core_belief.belief" :class="fieldClass" rows="2" :disabled="busy" @change="commit" />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.authorVerdict") }}</label>
        <select
          v-model="draft.core_belief.author_verdict"
          class="h-8 w-full rounded-md border bg-background px-2 text-sm"
          :disabled="busy"
          @change="commit"
        >
          <option value="">{{ t("character.verdictUnset") }}</option>
          <option value="prove">{{ t("character.verdictProve") }}</option>
          <option value="disprove">{{ t("character.verdictDisprove") }}</option>
          <option value="unresolved">{{ t("character.verdictUnresolved") }}</option>
        </select>
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.beliefSource") }}</label>
        <Textarea v-model="draft.core_belief.source" :class="fieldClass" rows="1" :disabled="busy" @change="commit" />
      </div>
    </section>

    <!-- 深层维度 -->
    <section class="space-y-3 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secDeep") }}</p>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.desire") }}</p>
        <Input v-model="draft.deep.desire_surface" class="h-8" :placeholder="t('character.desireSurface')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.desire_deep" class="h-8" :placeholder="t('character.desireDeep')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.fear") }}</p>
        <Input v-model="draft.deep.fear" class="h-8" :placeholder="t('character.fearWhat')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.fear_source" class="h-8" :placeholder="t('character.fearSource')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.secret") }}</p>
        <Textarea v-model="draft.deep.secret_content" :class="fieldClass" rows="1" :placeholder="t('character.secretContent')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.secret_who_knows" class="h-8" :placeholder="t('character.secretWho')" :disabled="busy" @change="commit" />
        <div>
          <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.secretShouldKnow") }}</label>
          <Input
            v-model="draft.deep.secret_should_know"
            class="h-8"
            list="char-name-opts"
            :disabled="busy"
            @change="commit"
          />
        </div>
        <Textarea v-model="draft.deep.secret_exposure" :class="fieldClass" rows="1" :placeholder="t('character.secretExposure')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.bottomLine") }}</p>
        <Input v-model="draft.deep.line_trigger" class="h-8" :placeholder="t('character.lineTrigger')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.line_source" class="h-8" :placeholder="t('character.lineSource')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.line_reaction" class="h-8" :placeholder="t('character.lineReaction')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.trauma") }}</p>
        <Input v-model="draft.deep.trauma_wound" class="h-8" :placeholder="t('character.traumaWound')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.trauma_trigger" class="h-8" :placeholder="t('character.traumaTrigger')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.trauma_stress" class="h-8" :placeholder="t('character.traumaStress')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.trauma_imprint" class="h-8" :placeholder="t('character.traumaImprint')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.contradiction") }}</p>
        <Input v-model="draft.deep.contradiction_poles" class="h-8" :placeholder="t('character.contraPoles')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.contradiction_source" class="h-8" :placeholder="t('character.contraSource')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.contradiction_trajectory" class="h-8" :placeholder="t('character.contraTrajectory')" :disabled="busy" @change="commit" />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.arc") }}</p>
        <Input v-model="draft.deep.arc_growth" class="h-8" :placeholder="t('character.arcGrowth')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.arc_fall" class="h-8" :placeholder="t('character.arcFall')" :disabled="busy" @change="commit" />
        <Input v-model="draft.deep.arc_choice" class="h-8" :placeholder="t('character.arcChoice')" :disabled="busy" @change="commit" />
      </div>
    </section>

    <!-- 角色声线 -->
    <section class="space-y-3 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.secVoice") }}</p>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.voicePos") }}</label>
        <Textarea
          v-model="draft.voice.positioning"
          :class="fieldClass"
          rows="2"
          :placeholder="t('character.voicePosPh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.cognitiveFilter") }}</label>
        <Textarea
          v-model="draft.voice.cognitive_filter"
          :class="fieldClass"
          rows="2"
          :placeholder="t('character.cognitiveFilterPh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.bodyLanguage") }}</label>
        <Textarea
          v-model="draft.voice.body_language"
          :class="fieldClass"
          rows="2"
          :placeholder="t('character.bodyLanguagePh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.syntax") }}</p>
        <Input v-model="draft.voice.sentence_length" class="h-8" :placeholder="t('character.sentenceLength')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.pause" class="h-8" :placeholder="t('character.pause')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.patterns" class="h-8" :placeholder="t('character.patterns')" :disabled="busy" @change="commit" />
        <div class="flex items-center justify-between gap-2">
          <label class="text-[11px] text-muted-foreground">{{ t("character.catchphrases") }}</label>
          <Button type="button" size="sm" variant="ghost" class="h-6 px-1.5 text-[11px]" :disabled="busy" @click="addCatchphrase">
            <Plus class="mr-0.5 h-3 w-3" />
            {{ t("character.addPhrase") }}
          </Button>
        </div>
        <div v-for="(_, i) in draft.voice.catchphrases" :key="i" class="flex items-center gap-2">
          <Input v-model="draft.voice.catchphrases[i]" class="h-8" :disabled="busy" @change="commit" />
          <Button type="button" size="sm" variant="ghost" class="h-7 px-1.5" :disabled="busy" @click="removeCatchphrase(i)">
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
      <div class="space-y-2">
        <p class="text-[11px] font-medium">{{ t("character.emotion") }}</p>
        <Input v-model="draft.voice.emotion_anger" class="h-8" :placeholder="t('character.emAnger')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.emotion_tense" class="h-8" :placeholder="t('character.emTense')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.emotion_mask" class="h-8" :placeholder="t('character.emMask')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.emotion_sad" class="h-8" :placeholder="t('character.emSad')" :disabled="busy" @change="commit" />
        <Input v-model="draft.voice.emotion_happy" class="h-8" :placeholder="t('character.emHappy')" :disabled="busy" @change="commit" />
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.banned") }}</label>
        <Textarea
          v-model="draft.voice.banned"
          :class="fieldClass"
          rows="2"
          :placeholder="t('character.bannedPh')"
          :disabled="busy"
          @change="commit"
        />
      </div>
    </section>

    <section class="space-y-3 rounded-md border bg-muted/20 p-3">
      <p class="text-xs font-medium text-muted-foreground">{{ t("character.sheetTitle") }}</p>
      <p class="text-[11px] text-muted-foreground">{{ t("character.sheetHint") }}</p>
      <div class="overflow-hidden rounded-md border bg-background">
        <img
          v-if="sheetUrl"
          :src="sheetUrl"
          :alt="t('character.sheetTitle')"
          class="max-h-72 w-full object-contain"
        />
        <div
          v-else
          class="grid grid-cols-4 divide-x text-center text-[10px] text-muted-foreground"
        >
          <div class="flex h-24 items-center justify-center">{{ t("character.sheetFront") }}</div>
          <div class="flex h-24 items-center justify-center">{{ t("character.sheetLeft") }}</div>
          <div class="flex h-24 items-center justify-center">{{ t("character.sheetBack") }}</div>
          <div class="flex h-24 items-center justify-center">{{ t("character.sheetRight") }}</div>
        </div>
      </div>
      <div>
        <label class="mb-1 block text-[11px] text-muted-foreground">{{ t("character.sheetPrompt") }}</label>
        <Textarea
          v-model="draft.sheet.prompt"
          :class="fieldClass"
          rows="3"
          :placeholder="t('character.sheetPromptPh')"
          :disabled="busy || sheetBusy"
          @change="commit"
        />
      </div>
      <p v-if="sheetError" class="text-xs text-destructive">{{ sheetError }}</p>
      <div class="flex flex-wrap justify-end gap-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          :disabled="busy || sheetBusy || sheetPromptBusy"
          @click="draftSheetPrompt"
        >
          <Loader2 v-if="sheetPromptBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
          {{ t("character.sheetDraft") }}
        </Button>
        <Button
          type="button"
          size="sm"
          :disabled="busy || sheetBusy || sheetPromptBusy"
          @click="generateSheet"
        >
          <Loader2 v-if="sheetBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
          {{ sheetBusy ? t("character.sheetBusy") : t("character.sheetGenerate") }}
        </Button>
      </div>
    </section>

    <div class="flex justify-end pb-2">
      <Button size="sm" :disabled="busy" @click="commit">
        {{ busy ? t("character.saving") : t("character.save") }}
      </Button>
    </div>

    <!-- AI modal -->
    <div
      v-if="pendingAi"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelAi"
    >
      <div class="w-full max-w-md space-y-3 rounded-xl border bg-card p-4 shadow-lg">
        <h4 class="text-sm font-semibold">{{ t("character.aiTitle", { field: aiFieldLabel }) }}</h4>
        <Textarea
          v-model="aiPrompt"
          rows="4"
          class="text-sm"
          :placeholder="t('character.aiPh')"
          :disabled="aiBusy"
        />
        <p v-if="aiError" class="text-xs text-destructive">{{ aiError }}</p>
        <div class="flex justify-end gap-2">
          <Button type="button" size="sm" variant="ghost" :disabled="aiBusy" @click="cancelAi">
            {{ t("character.aiCancel") }}
          </Button>
          <Button type="button" size="sm" :disabled="aiBusy || !aiPrompt.trim()" @click="confirmAi">
            <Loader2 v-if="aiBusy" class="mr-1 h-3.5 w-3.5 animate-spin" />
            {{ aiBusy ? t("character.aiBusy") : t("character.aiConfirm") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
