<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  api,
  type CompatProviderView,
  type McpStatus,
  type SettingsView,
  type SkillMatchPreview,
  type SkillPreviewItem,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Eye, EyeOff } from "@lucide/vue";
import { LOCALE_OPTIONS, setLocalePreference, useI18n, type LocalePreference } from "@/i18n";

type DraftProvider = {
  id: string;
  label: string;
  protocol: string;
  base_url: string;
  api_key: string;
  models: string[];
  api_key_configured: boolean;
  api_key_masked: string;
};

const { t } = useI18n();
const uiLocale = ref<LocalePreference>("system");
const mcpPort = ref(17832);
const mcpEnabled = ref(true);
const mcpLan = ref(false);
const mcpStatus = ref<McpStatus | null>(null);
const mcpBusy = ref(false);
const settings = ref<SettingsView | null>(null);
const providers = ref<DraftProvider[]>([]);
const geminiKey = ref("");
const claudeKey = ref("");
const show = ref<Record<string, boolean>>({});
const msg = ref("");
const err = ref("");
const saving = ref(false);
const refreshBusyId = ref<string | null>(null);

const adding = ref(false);
const addLabel = ref("");
const addProtocol = ref("openai");
const addBaseUrl = ref("https://api.deepseek.com");
const addKey = ref("");
const addFetched = ref<string[]>([]);
const addSelected = ref<Set<string>>(new Set());
const addBusy = ref(false);
const addErr = ref("");

const skillsRoot = ref("");
const skillsBundled = ref("");
const skillsUser = ref("");
const skillsExists = ref(false);
const skills = ref<SkillPreviewItem[]>([]);
const skillsBusy = ref(false);
const skillsErr = ref("");
const expandedSkill = ref<string | null>(null);
const skillQuery = ref("");
const skillMatchBusy = ref(false);
const skillMatch = ref<SkillMatchPreview | null>(null);

function fromView(p: CompatProviderView): DraftProvider {
  return {
    id: p.id,
    label: p.label,
    protocol: p.protocol || "openai",
    base_url: p.base_url,
    api_key: "",
    models: [...(p.models || [])],
    api_key_configured: p.api_key_configured,
    api_key_masked: p.api_key_masked,
  };
}

async function load() {
  const s = await api.getSettings();
  settings.value = s;
  providers.value = (s.compat_providers || []).map(fromView);
  uiLocale.value = (s.ui_locale || "system") as LocalePreference;
  mcpPort.value = s.mcp_port || 17832;
  mcpEnabled.value = s.mcp_enabled !== false;
  mcpLan.value = !!s.mcp_lan;
  setLocalePreference(uiLocale.value);
  geminiKey.value = "";
  claudeKey.value = "";
  void refreshMcpStatus();
}

async function refreshMcpStatus() {
  try {
    mcpStatus.value = await api.getMcpStatus();
  } catch {
    mcpStatus.value = null;
  }
}

async function restartMcp() {
  mcpBusy.value = true;
  err.value = "";
  try {
    mcpStatus.value = await api.restartMcpServer();
    msg.value = t("mcp.saved");
  } catch (e) {
    err.value = String(e);
  } finally {
    mcpBusy.value = false;
  }
}

async function loadSkills() {
  skillsBusy.value = true;
  skillsErr.value = "";
  skillMatch.value = null;
  try {
    const p = await api.listChatSkills();
    skillsRoot.value = p.root;
    skillsBundled.value = p.bundled || "";
    skillsUser.value = p.user || p.root || "";
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
  void api.refreshModelCatalog().then(() => load());
  void loadSkills();
});

function toggle(id: string) {
  show.value = { ...show.value, [id]: !show.value[id] };
}

function opt(v: string) {
  const trimmed = v.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function openAdd() {
  adding.value = true;
  resetAddForm();
}

function resetAddForm() {
  addLabel.value = "";
  addProtocol.value = "openai";
  addBaseUrl.value = "https://api.deepseek.com";
  addKey.value = "";
  addFetched.value = [];
  addSelected.value = new Set();
  addErr.value = "";
}

function cancelAdd() {
  adding.value = false;
  resetAddForm();
}

async function fetchAddModels() {
  addErr.value = "";
  addBusy.value = true;
  try {
    const ids = await api.fetchCompatModels(addBaseUrl.value.trim(), addKey.value.trim());
    addFetched.value = ids;
    addSelected.value = new Set(ids);
    if (!ids.length) addErr.value = t("settings.compatNoModels");
  } catch (e) {
    addFetched.value = [];
    addSelected.value = new Set();
    addErr.value = String(e);
  } finally {
    addBusy.value = false;
  }
}

function toggleAddModel(id: string) {
  const next = new Set(addSelected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  addSelected.value = next;
}

function selectAllAdd(on: boolean) {
  addSelected.value = on ? new Set(addFetched.value) : new Set();
}

function confirmAdd() {
  addErr.value = "";
  if (!addLabel.value.trim() || !addBaseUrl.value.trim() || !addKey.value.trim()) {
    addErr.value = t("settings.compatNeedFields");
    return;
  }
  if (!addSelected.value.size) {
    addErr.value = t("settings.compatNeedModels");
    return;
  }
  providers.value = [
    ...providers.value,
    {
      id: crypto.randomUUID(),
      label: addLabel.value.trim(),
      protocol: addProtocol.value || "openai",
      base_url: addBaseUrl.value.trim(),
      api_key: addKey.value.trim(),
      models: [...addSelected.value].sort(),
      api_key_configured: true,
      api_key_masked: "",
    },
  ];
  cancelAdd();
}

function removeProvider(id: string) {
  providers.value = providers.value.filter((p) => p.id !== id);
}

async function refreshProvider(id: string) {
  refreshBusyId.value = id;
  err.value = "";
  try {
    // Persist draft first so backend has latest key/url if newly added
    await persistProvidersOnly();
    const ids = await api.refreshCompatProviderModels(id);
    const p = providers.value.find((x) => x.id === id);
    if (p) p.models = ids;
    await load();
  } catch (e) {
    err.value = String(e);
  } finally {
    refreshBusyId.value = null;
  }
}

async function persistProvidersOnly() {
  await api.saveSettings({
    compat_providers: providers.value.map((p) => ({
      id: p.id,
      label: p.label,
      protocol: p.protocol,
      base_url: p.base_url,
      api_key: opt(p.api_key),
      models: p.models,
    })),
  });
}

async function save() {
  err.value = "";
  msg.value = "";
  saving.value = true;
  try {
    const s = await api.saveSettings({
      compat_providers: providers.value.map((p) => ({
        id: p.id,
        label: p.label,
        protocol: p.protocol,
        base_url: p.base_url,
        api_key: opt(p.api_key),
        models: p.models,
      })),
      gemini_api_key: opt(geminiKey.value),
      claude_api_key: opt(claudeKey.value),
      ui_locale: uiLocale.value,
      mcp_port: Number(mcpPort.value) || 17832,
      mcp_enabled: mcpEnabled.value,
      mcp_lan: mcpLan.value,
    });
    settings.value = s;
    setLocalePreference((s.ui_locale || "system") as LocalePreference);
    mcpPort.value = s.mcp_port || 17832;
    mcpEnabled.value = s.mcp_enabled !== false;
    mcpLan.value = !!s.mcp_lan;
    void refreshMcpStatus();
    msg.value = t("settings.saved");
    await api.refreshModelCatalog();
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

const scrollRoot = ref<HTMLElement | null>(null);
const activeSection = ref("language");

const settingsSections = computed(() => [
  { id: "language", label: t("settings.language") },
  { id: "skills", label: t("settings.skills") },
  { id: "compat", label: t("settings.compatTitle") },
  { id: "gemini", label: "Gemini" },
  { id: "claude", label: "Claude" },
]);

function scrollToSection(id: string) {
  activeSection.value = id;
  const el = scrollRoot.value?.querySelector(`#settings-${id}`);
  el?.scrollIntoView({ behavior: "smooth", block: "start" });
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <nav
      class="flex w-36 shrink-0 flex-col gap-0.5 overflow-y-auto border-r bg-muted/20 p-2"
      :aria-label="t('settings.navAria')"
    >
      <button
        v-for="sec in settingsSections"
        :key="sec.id"
        type="button"
        class="rounded-md px-2.5 py-2 text-left text-xs leading-snug transition-colors"
        :class="
          activeSection === sec.id
            ? 'bg-accent font-medium text-accent-foreground'
            : 'text-muted-foreground hover:bg-muted hover:text-foreground'
        "
        @click="scrollToSection(sec.id)"
      >
        {{ sec.label }}
      </button>
    </nav>

    <div class="flex min-h-0 min-w-0 flex-1 flex-col">
      <div class="shrink-0 border-b bg-background px-6 py-4">
        <div class="mx-auto flex max-w-xl items-start justify-between gap-4">
          <div class="min-w-0">
            <h1 class="text-2xl font-semibold tracking-tight">{{ t("settings.title") }}</h1>
            <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.subtitle") }}</p>
          </div>
          <Button class="shrink-0" :disabled="saving" @click="save">
            {{ saving ? t("settings.saving") : t("settings.save") }}
          </Button>
        </div>
        <div v-if="msg || err" class="mx-auto mt-2 max-w-xl space-y-1">
          <p v-if="msg" class="text-sm text-primary">{{ msg }}</p>
          <p v-if="err" class="text-sm text-destructive">{{ err }}</p>
        </div>
      </div>

      <div
        ref="scrollRoot"
        class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 py-6"
      >
        <div class="mx-auto max-w-xl space-y-6">
    <Card id="settings-language" class="scroll-mt-4">
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

    <Card id="settings-mcp" class="scroll-mt-4">
      <CardHeader>
        <CardTitle>{{ t("mcp.title") }}</CardTitle>
        <p class="text-sm text-muted-foreground">
          {{ t("mcp.hint", { port: mcpPort || 17832 }) }}
        </p>
      </CardHeader>
      <CardContent class="space-y-4">
        <label class="flex items-center gap-2 text-sm">
          <input v-model="mcpEnabled" type="checkbox" class="h-4 w-4" />
          {{ t("mcp.enabled") }}
        </label>
        <label class="flex items-center gap-2 text-sm">
          <input v-model="mcpLan" type="checkbox" class="h-4 w-4" />
          {{ t("mcp.lan") }}
        </label>
        <p v-if="mcpLan" class="text-xs text-muted-foreground">{{ t("mcp.lanHint") }}</p>
        <div class="space-y-1">
          <label class="text-xs text-muted-foreground">{{ t("mcp.port") }}</label>
          <Input v-model.number="mcpPort" type="number" min="1024" max="65535" class="h-9" />
        </div>
        <div class="rounded-md border bg-muted/30 px-3 py-2 text-xs">
          <p>
            {{ t("mcp.endpoint") }}:
            <span class="font-mono">{{ mcpStatus?.endpoint || `http://127.0.0.1:${mcpPort}/mcp` }}</span>
          </p>
          <p v-if="mcpLan || mcpStatus?.lan_enabled" class="mt-1">
            {{ t("mcp.lanEndpoint") }}:
            <span class="font-mono">{{ mcpStatus?.lan_endpoint || `http://<LAN-IP>:${mcpPort}/mcp` }}</span>
          </p>
          <p class="mt-1" :class="mcpStatus?.running ? 'text-primary' : 'text-muted-foreground'">
            {{ mcpStatus?.running ? t("mcp.statusRunning") : t("mcp.statusStopped") }}
            <template v-if="mcpStatus?.error"> · {{ mcpStatus.error }}</template>
          </p>
        </div>
        <Button variant="outline" size="sm" :disabled="mcpBusy || saving" @click="restartMcp">
          {{ mcpBusy ? t("settings.saving") : t("mcp.restart") }}
        </Button>
      </CardContent>
    </Card>

    <Card id="settings-skills" class="scroll-mt-4">
      <CardHeader>
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <CardTitle>{{ t("settings.skills") }}</CardTitle>
            <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.skillsHint") }}</p>
            <p class="mt-1 truncate font-mono text-xs text-muted-foreground" :title="skillsBundled">
              {{ t("settings.skillsBundled") }} · {{ skillsBundled || "…" }}
            </p>
            <p class="mt-1 truncate font-mono text-xs text-muted-foreground" :title="skillsUser || skillsRoot">
              {{ t("settings.skillsUser") }} · {{ skillsUser || skillsRoot || "…" }}
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
                  v-if="s.builtin"
                  class="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                >
                  {{ t("settings.skillsBuiltin") }}
                </span>
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

    <Card id="settings-compat" class="scroll-mt-4">
      <CardHeader>
        <div class="flex items-start justify-between gap-3">
          <div>
            <CardTitle>{{ t("settings.compatTitle") }}</CardTitle>
            <p class="mt-1 text-sm text-muted-foreground">{{ t("settings.compatHint") }}</p>
          </div>
          <Button v-if="!adding" variant="outline" size="sm" @click="openAdd">
            {{ t("settings.compatAdd") }}
          </Button>
        </div>
      </CardHeader>
      <CardContent class="space-y-4">
        <div v-if="adding" class="space-y-3 rounded-md border border-border p-3">
          <div>
            <label class="mb-1 block text-sm">{{ t("settings.compatProtocol") }}</label>
            <select
              v-model="addProtocol"
              class="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
            >
              <option value="openai">OpenAI</option>
            </select>
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("settings.compatLabel") }}</label>
            <Input v-model="addLabel" :placeholder="t('settings.compatLabelPh')" />
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("settings.baseUrl") }}</label>
            <Input v-model="addBaseUrl" placeholder="https://api.deepseek.com" />
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("settings.apiKey") }}</label>
            <div class="flex gap-2">
              <Input
                v-model="addKey"
                :type="show.add ? 'text' : 'password'"
                :placeholder="`sk-… (${t('settings.keyPlaceholder')})`"
                class="flex-1"
              />
              <Button variant="outline" size="icon" type="button" @click="toggle('add')">
                <Eye v-if="!show.add" class="h-4 w-4" />
                <EyeOff v-else class="h-4 w-4" />
              </Button>
            </div>
          </div>
          <div class="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" :disabled="addBusy" @click="fetchAddModels">
              {{ addBusy ? t("settings.compatFetching") : t("settings.compatFetch") }}
            </Button>
            <Button
              v-if="addFetched.length"
              variant="outline"
              size="sm"
              type="button"
              @click="selectAllAdd(addSelected.size < addFetched.length)"
            >
              {{
                addSelected.size < addFetched.length
                  ? t("settings.compatSelectAll")
                  : t("settings.compatSelectNone")
              }}
            </Button>
          </div>
          <p v-if="addErr" class="text-sm text-destructive">{{ addErr }}</p>
          <ul
            v-if="addFetched.length"
            class="max-h-48 space-y-1 overflow-y-auto rounded border border-border p-2"
          >
            <li v-for="id in addFetched" :key="id" class="flex items-center gap-2 text-sm">
              <input
                :id="`add-m-${id}`"
                type="checkbox"
                class="h-4 w-4"
                :checked="addSelected.has(id)"
                @change="toggleAddModel(id)"
              />
              <label :for="`add-m-${id}`" class="font-mono text-xs">{{ id }}</label>
            </li>
          </ul>
          <div class="flex gap-2">
            <Button size="sm" @click="confirmAdd">{{ t("settings.compatConfirm") }}</Button>
            <Button variant="outline" size="sm" @click="cancelAdd">{{ t("settings.compatCancel") }}</Button>
          </div>
        </div>

        <p v-if="!providers.length && !adding" class="text-sm text-muted-foreground">
          {{ t("settings.compatEmpty") }}
        </p>
        <ul v-else class="space-y-3">
          <li
            v-for="p in providers"
            :key="p.id"
            class="space-y-2 rounded-md border border-border px-3 py-2"
          >
            <div class="flex items-start justify-between gap-2">
              <div class="min-w-0">
                <p class="font-medium">{{ p.label || p.id }}</p>
                <p class="truncate text-xs text-muted-foreground">
                  {{ p.protocol }} · {{ p.base_url }}
                </p>
                <p class="mt-1 text-xs text-muted-foreground">
                  {{ t("settings.status") }}
                  <span :class="p.api_key_configured || p.api_key ? 'text-primary' : 'text-muted-foreground'">
                    {{
                      statusText(
                        p.api_key_configured || !!p.api_key,
                        p.api_key_masked || undefined,
                      )
                    }}
                  </span>
                  · {{ t("settings.compatModelCount", { n: p.models.length }) }}
                </p>
              </div>
              <div class="flex shrink-0 gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  :disabled="refreshBusyId === p.id"
                  @click="refreshProvider(p.id)"
                >
                  {{
                    refreshBusyId === p.id
                      ? t("settings.compatRefreshing")
                      : t("settings.compatRefresh")
                  }}
                </Button>
                <Button variant="outline" size="sm" @click="removeProvider(p.id)">
                  {{ t("settings.compatRemove") }}
                </Button>
              </div>
            </div>
            <p
              v-if="p.models.length"
              class="line-clamp-2 font-mono text-[11px] text-muted-foreground"
            >
              {{ p.models.join(", ") }}
            </p>
          </li>
        </ul>
      </CardContent>
    </Card>

    <Card id="settings-gemini" class="scroll-mt-4">
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

    <Card id="settings-claude" class="scroll-mt-4">
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
        </div>
      </div>
    </div>
  </div>
</template>
