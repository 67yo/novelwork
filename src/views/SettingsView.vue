<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  api,
  type ModelCatalog,
  type SettingsView,
  type SkillMatchPreview,
  type SkillPreviewItem,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Eye, EyeOff } from "@lucide/vue";
import { LOCALE_OPTIONS, setLocalePreference, useI18n, type LocalePreference } from "@/i18n";

const { t } = useI18n();
const uiLocale = ref<LocalePreference>("system");
const settings = ref<SettingsView | null>(null);
const catalog = ref<ModelCatalog | null>(null);
const deepseekKey = ref("");
const chatgptKey = ref("");
const geminiKey = ref("");
const claudeKey = ref("");
const grokKey = ref("");
const baseUrl = ref("https://api.deepseek.com");
const createModel = ref("deepseek-v4-flash");
const generateModel = ref("deepseek-v4-flash");
const chatModel = ref("deepseek-v4-flash");
const refineModel = ref("deepseek-reasoner");
const knowledgeModel = ref("deepseek-v4-flash");
const show = ref<Record<string, boolean>>({});
const msg = ref("");
const err = ref("");
const saving = ref(false);
const catalogBusy = ref(false);

const skillsRoot = ref("");
const skillsExists = ref(false);
const skills = ref<SkillPreviewItem[]>([]);
const skillsBusy = ref(false);
const skillsErr = ref("");
const expandedSkill = ref<string | null>(null);
const skillQuery = ref("");
const skillMatchBusy = ref(false);
const skillMatch = ref<SkillMatchPreview | null>(null);

const liveIds = computed(() => {
  const c = catalog.value;
  if (!c) return new Set<string>();
  return new Set([...c.deepseek, ...c.gemini, ...c.claude, ...c.grok]);
});

/** 支持的模型 + 当前选用但不在目录里的（标废弃） */
function optionsFor(selected: string) {
  const live = liveIds.value;
  const ids = [...live].sort();
  const rows = ids.map((id) => ({ id, label: id }));
  if (selected && !live.has(selected)) {
    rows.unshift({ id: selected, label: `${selected}${t("settings.deprecated")}` });
  }
  return rows;
}

async function load() {
  const s = await api.getSettings();
  settings.value = s;
  catalog.value = s.model_catalog;
  baseUrl.value = s.deepseek_base_url;
  createModel.value = s.create_model || "deepseek-v4-flash";
  generateModel.value = s.generate_model || "deepseek-v4-flash";
  chatModel.value = s.chat_model || "deepseek-v4-flash";
  refineModel.value = s.refine_model || "deepseek-reasoner";
  knowledgeModel.value = s.knowledge_model || "deepseek-v4-flash";
  uiLocale.value = (s.ui_locale || "system") as LocalePreference;
  setLocalePreference(uiLocale.value);
  deepseekKey.value = "";
  chatgptKey.value = "";
  geminiKey.value = "";
  claudeKey.value = "";
  grokKey.value = "";
}

async function refreshCatalog() {
  catalogBusy.value = true;
  try {
    catalog.value = await api.refreshModelCatalog();
  } catch (e) {
    err.value = String(e);
  } finally {
    catalogBusy.value = false;
  }
}

async function loadSkills() {
  skillsBusy.value = true;
  skillsErr.value = "";
  skillMatch.value = null;
  try {
    const p = await api.listChatSkills();
    skillsRoot.value = p.root;
    skillsExists.value = p.exists;
    skills.value = p.skills;
    if (expandedSkill.value && !p.skills.some((s) => s.name === expandedSkill.value)) {
      expandedSkill.value = null;
    }
  } catch (e) {
    skillsErr.value = String(e);
  } finally {
    skillsBusy.value = false;
  }
}

function toggleSkill(name: string) {
  expandedSkill.value = expandedSkill.value === name ? null : name;
}

async function trySkillMatch() {
  skillMatchBusy.value = true;
  try {
    skillMatch.value = await api.previewChatSkillMatch(skillQuery.value);
  } catch (e) {
    skillsErr.value = String(e);
  } finally {
    skillMatchBusy.value = false;
  }
}

function skillMatchText(m: SkillMatchPreview): string {
  if (m.via === "empty") return t("settings.skillsMatchEmpty");
  if (m.via === "no_dir") return t("settings.skillsMatchNoDir");
  if (m.via === "trigger_only" && m.name) {
    return t("settings.skillsMatchTriggerOnly", { name: m.name });
  }
  if (m.matched && m.name) {
    if (m.via === "at") return t("settings.skillsMatchAt", { name: m.name });
    const score = m.score != null ? m.score.toFixed(1) : "–";
    return t("settings.skillsMatchHit", { name: m.name, score });
  }
  return t("settings.skillsMatchNone");
}

onMounted(async () => {
  await load();
  // 打开设置页再拉一次，覆盖启动任务尚未完成的情况
  refreshCatalog().then(() => load());
  void loadSkills();
});

function toggle(id: string) {
  show.value = { ...show.value, [id]: !show.value[id] };
}

function opt(v: string) {
  const trimmed = v.trim();
  return trimmed.length > 0 ? trimmed : null;
}

async function save() {
  err.value = "";
  msg.value = "";
  saving.value = true;
  try {
    const s = await api.saveSettings({
      deepseek_api_key: opt(deepseekKey.value),
      chatgpt_api_key: opt(chatgptKey.value),
      gemini_api_key: opt(geminiKey.value),
      claude_api_key: opt(claudeKey.value),
      grok_api_key: opt(grokKey.value),
      deepseek_base_url: baseUrl.value.trim(),
      create_model: createModel.value.trim(),
      generate_model: generateModel.value.trim(),
      chat_model: chatModel.value.trim(),
      refine_model: refineModel.value.trim(),
      knowledge_model: knowledgeModel.value.trim(),
      ui_locale: uiLocale.value,
    });
    settings.value = s;
    catalog.value = s.model_catalog;
    setLocalePreference((s.ui_locale || "system") as LocalePreference);
    deepseekKey.value = "";
    chatgptKey.value = "";
    geminiKey.value = "";
    claudeKey.value = "";
    grokKey.value = "";
    msg.value = t("settings.saved");
    // 新 Key 保存后立刻刷新目录
    await refreshCatalog();
    await load();
  } catch (e) {
    err.value = String(e);
  } finally {
    saving.value = false;
  }
}

function statusText(configured: boolean | undefined, masked: string | undefined) {
  return configured
    ? t("settings.configured", { masked: masked ?? "" })
    : t("settings.notConfigured");
}

function onLocaleChange() {
  setLocalePreference(uiLocale.value);
}

const catalogHint = computed(() => {
  void t; // keep reactive via locale reads in t below
  const c = catalog.value;
  if (!c) return "";
  const parts = [
    c.deepseek.length && `DeepSeek ${c.deepseek.length}`,
    c.gemini.length && `Gemini ${c.gemini.length}`,
    c.claude.length && `Claude ${c.claude.length}`,
    c.grok.length && `Grok ${c.grok.length}`,
  ].filter(Boolean);
  const errKeys = Object.keys(c.errors || {});
  const fail = errKeys.length ? t("settings.catalogFailed", { keys: errKeys.join(", ") }) : "";
  const when = c.updated_at ? ` · ${c.updated_at.slice(0, 19).replace("T", " ")}` : "";
  const sep = fail ? ` · ${fail}` : "";
  return parts.length
    ? `${parts.join(" / ")}${when}${sep}`
    : `${t("settings.catalogWaiting")}${sep}`;
});
</script>

<template>
  <div class="mx-auto h-full max-w-xl space-y-6 overflow-y-auto overscroll-contain p-6">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("settings.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.subtitle") }}</p>
      </div>
      <Button class="shrink-0" :disabled="saving" @click="save">
        {{ saving ? t("settings.saving") : t("settings.save") }}
      </Button>
    </div>
    <div v-if="msg || err" class="space-y-1">
      <p v-if="msg" class="text-sm text-primary">{{ msg }}</p>
      <p v-if="err" class="text-sm text-destructive">{{ err }}</p>
    </div>

    <Card>
      <CardHeader>
        <CardTitle>{{ t("settings.language") }}</CardTitle>
        <p class="text-sm text-muted-foreground">{{ t("settings.languageHint") }}</p>
      </CardHeader>
      <CardContent>
        <select
          v-model="uiLocale"
          class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          @change="onLocaleChange"
        >
          <option v-for="opt in LOCALE_OPTIONS" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <CardTitle>{{ t("settings.skills") }}</CardTitle>
            <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.skillsHint") }}</p>
            <p class="mt-1 truncate font-mono text-xs text-muted-foreground" :title="skillsRoot">
              {{ skillsRoot || "…" }}
            </p>
          </div>
          <Button variant="outline" size="sm" class="shrink-0" :disabled="skillsBusy" @click="loadSkills">
            {{ skillsBusy ? t("settings.skillsRefreshing") : t("settings.skillsRefresh") }}
          </Button>
        </div>
      </CardHeader>
      <CardContent class="space-y-4">
        <p v-if="skillsErr" class="text-sm text-destructive">{{ skillsErr }}</p>
        <p v-else-if="skillsBusy" class="text-sm text-muted-foreground">{{ t("settings.skillsLoading") }}</p>
        <p v-else-if="!skillsExists" class="text-sm text-muted-foreground">{{ t("settings.skillsMissingDir") }}</p>
        <p v-else-if="skills.length === 0" class="text-sm text-muted-foreground">{{ t("settings.skillsEmpty") }}</p>
        <ul v-else class="max-h-80 space-y-2 overflow-y-auto overscroll-contain">
          <li
            v-for="s in skills"
            :key="s.name"
            class="rounded-md border border-border px-3 py-2"
          >
            <button type="button" class="w-full text-left" @click="toggleSkill(s.name)">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-mono text-sm font-medium">@{{ s.name }}</span>
                <span
                  v-if="s.trigger"
                  class="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                >
                  {{ t("settings.skillsTrigger") }}
                </span>
              </div>
              <p class="mt-1 line-clamp-2 text-xs text-muted-foreground">{{ s.description }}</p>
            </button>
            <pre
              v-if="expandedSkill === s.name"
              class="mt-2 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted/40 p-2 text-xs text-foreground"
            >{{ s.body_preview }}</pre>
          </li>
        </ul>
        <div class="space-y-2 border-t pt-4">
          <label class="block text-sm">{{ t("settings.skillsMatchLabel") }}</label>
          <div class="flex gap-2">
            <Input
              v-model="skillQuery"
              :placeholder="t('settings.skillsMatchPh')"
              class="flex-1"
              @keydown.enter.prevent="trySkillMatch"
            />
            <Button variant="outline" :disabled="skillMatchBusy" @click="trySkillMatch">
              {{ skillMatchBusy ? t("settings.skillsMatching") : t("settings.skillsMatch") }}
            </Button>
          </div>
          <p v-if="skillMatch" class="text-sm text-muted-foreground">{{ skillMatchText(skillMatch) }}</p>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <div class="flex items-start justify-between gap-3">
          <div>
            <CardTitle>{{ t("settings.models") }}</CardTitle>
            <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.modelsHint") }}</p>
            <p class="mt-1 text-xs text-muted-foreground">{{ catalogHint }}</p>
          </div>
          <Button variant="outline" size="sm" :disabled="catalogBusy" @click="refreshCatalog().then(load)">
            {{ catalogBusy ? t("settings.refreshing") : t("settings.refreshCatalog") }}
          </Button>
        </div>
      </CardHeader>
      <CardContent class="space-y-4">
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.createModel") }}</label>
          <select
            v-model="createModel"
            class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in optionsFor(createModel)" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.generateModel") }}</label>
          <select
            v-model="generateModel"
            class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in optionsFor(generateModel)" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.chatModel") }}</label>
          <select
            v-model="chatModel"
            class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in optionsFor(chatModel)" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.refineModel") }}</label>
          <select
            v-model="refineModel"
            class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in optionsFor(refineModel)" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
          <p class="mt-1 text-xs text-muted-foreground">{{ t("settings.refineHint") }}</p>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.knowledgeModel") }}</label>
          <select
            v-model="knowledgeModel"
            class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in optionsFor(knowledgeModel)" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
          <p class="mt-1 text-xs text-muted-foreground">{{ t("settings.knowledgeHint") }}</p>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>DeepSeek</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("settings.status") }}
          <span :class="settings?.deepseek_api_key_configured ? 'text-primary' : 'text-muted-foreground'">
            {{ statusText(settings?.deepseek_api_key_configured, settings?.deepseek_api_key_masked) }}
          </span>
        </p>
      </CardHeader>
      <CardContent class="space-y-4">
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
          <div class="flex gap-2">
            <Input
              v-model="deepseekKey"
              :type="show.deepseek ? 'text' : 'password'"
              :placeholder="`sk-… (${t('settings.keyPlaceholder')})`"
              class="flex-1"
            />
            <Button variant="outline" size="icon" type="button" @click="toggle('deepseek')">
              <Eye v-if="!show.deepseek" class="h-4 w-4" />
              <EyeOff v-else class="h-4 w-4" />
            </Button>
          </div>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("settings.baseUrl") }}</label>
          <Input v-model="baseUrl" placeholder="https://api.deepseek.com" />
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>ChatGPT</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("settings.status") }}
          <span :class="settings?.chatgpt_api_key_configured ? 'text-primary' : 'text-muted-foreground'">
            {{ statusText(settings?.chatgpt_api_key_configured, settings?.chatgpt_api_key_masked) }}
          </span>
        </p>
      </CardHeader>
      <CardContent>
        <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
        <div class="flex gap-2">
          <Input
            v-model="chatgptKey"
            :type="show.chatgpt ? 'text' : 'password'"
            :placeholder="`sk-… (${t('settings.keyPlaceholder')})`"
            class="flex-1"
          />
          <Button variant="outline" size="icon" type="button" @click="toggle('chatgpt')">
            <Eye v-if="!show.chatgpt" class="h-4 w-4" />
            <EyeOff v-else class="h-4 w-4" />
          </Button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Gemini</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("settings.status") }}
          <span :class="settings?.gemini_api_key_configured ? 'text-primary' : 'text-muted-foreground'">
            {{ statusText(settings?.gemini_api_key_configured, settings?.gemini_api_key_masked) }}
          </span>
        </p>
      </CardHeader>
      <CardContent>
        <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
        <div class="flex gap-2">
          <Input
            v-model="geminiKey"
            :type="show.gemini ? 'text' : 'password'"
            :placeholder="`AIza… (${t('settings.keyPlaceholder')})`"
            class="flex-1"
          />
          <Button variant="outline" size="icon" type="button" @click="toggle('gemini')">
            <Eye v-if="!show.gemini" class="h-4 w-4" />
            <EyeOff v-else class="h-4 w-4" />
          </Button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Claude</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("settings.status") }}
          <span :class="settings?.claude_api_key_configured ? 'text-primary' : 'text-muted-foreground'">
            {{ statusText(settings?.claude_api_key_configured, settings?.claude_api_key_masked) }}
          </span>
        </p>
      </CardHeader>
      <CardContent>
        <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
        <div class="flex gap-2">
          <Input
            v-model="claudeKey"
            :type="show.claude ? 'text' : 'password'"
            :placeholder="`sk-ant-… (${t('settings.keyPlaceholder')})`"
            class="flex-1"
          />
          <Button variant="outline" size="icon" type="button" @click="toggle('claude')">
            <Eye v-if="!show.claude" class="h-4 w-4" />
            <EyeOff v-else class="h-4 w-4" />
          </Button>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Grok</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("settings.status") }}
          <span :class="settings?.grok_api_key_configured ? 'text-primary' : 'text-muted-foreground'">
            {{ statusText(settings?.grok_api_key_configured, settings?.grok_api_key_masked) }}
          </span>
        </p>
      </CardHeader>
      <CardContent>
        <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
        <div class="flex gap-2">
          <Input
            v-model="grokKey"
            :type="show.grok ? 'text' : 'password'"
            :placeholder="`xai-… (${t('settings.keyPlaceholder')})`"
            class="flex-1"
          />
          <Button variant="outline" size="icon" type="button" @click="toggle('grok')">
            <Eye v-if="!show.grok" class="h-4 w-4" />
            <EyeOff v-else class="h-4 w-4" />
          </Button>
        </div>
      </CardContent>
    </Card>

    <div class="space-y-2">
      <p v-if="msg" class="text-sm text-primary">{{ msg }}</p>
      <p v-if="err" class="text-sm text-destructive">{{ err }}</p>
      <Button :disabled="saving" @click="save">{{ saving ? t("settings.saving") : t("settings.save") }}</Button>
    </div>
  </div>
</template>
