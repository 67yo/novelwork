<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { RouterLink, RouterView, useRoute } from "vue-router";
import { api } from "@/lib/api";
import { setLocalePreference, useI18n, type LocalePreference } from "@/i18n";
import { BookOpen, Bot, ChartColumnStacked, Library, Settings, Sparkles } from "@lucide/vue";

const route = useRoute();
const keyOk = ref(false);
const { t } = useI18n();

async function refreshKey() {
  try {
    const s = await api.getSettings();
    keyOk.value =
      s.deepseek_api_key_configured ||
      s.chatgpt_api_key_configured ||
      s.gemini_api_key_configured ||
      s.claude_api_key_configured ||
      s.grok_api_key_configured;
    setLocalePreference((s.ui_locale || "system") as LocalePreference);
  } catch {
    keyOk.value = false;
  }
}

onMounted(refreshKey);
watch(() => route.path, refreshKey);

function navClass(path: string) {
  const active =
    path === "/novels" ? route.path.startsWith("/novels") : route.path === path || route.path.startsWith(`${path}/`);
  return [
    "flex w-full flex-col items-center gap-0.5 rounded-md px-1 py-2 text-[11px] leading-tight transition-colors",
    active ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
  ];
}
</script>

<template>
  <div class="flex h-screen overflow-hidden">
    <aside
      class="z-50 flex w-16 shrink-0 flex-col border-r bg-[linear-gradient(180deg,oklch(0.97_0.02_155),oklch(0.99_0.01_85))]"
    >
      <RouterLink
        to="/novels"
        class="flex flex-col items-center gap-0.5 border-b px-1 py-3 text-primary"
        :title="'Nove Work'"
      >
        <Sparkles class="h-5 w-5" />
        <span class="text-[10px] font-semibold leading-tight tracking-tight">Nove</span>
      </RouterLink>
      <nav class="flex flex-1 flex-col gap-1 p-1.5">
        <RouterLink :to="'/novels'" :class="navClass('/novels')" :title="t('nav.novels')">
          <BookOpen class="h-4 w-4" />
          <span>{{ t("nav.novels") }}</span>
        </RouterLink>
        <RouterLink :to="'/library'" :class="navClass('/library')" :title="t('nav.library')">
          <Library class="h-4 w-4" />
          <span>{{ t("nav.library") }}</span>
        </RouterLink>
        <RouterLink :to="'/stats'" :class="navClass('/stats')" :title="t('nav.stats')">
          <ChartColumnStacked class="h-4 w-4" />
          <span>{{ t("nav.stats") }}</span>
        </RouterLink>
        <RouterLink :to="'/settings'" :class="navClass('/settings')" :title="t('nav.settings')">
          <Settings class="h-4 w-4" />
          <span>{{ t("nav.settings") }}</span>
        </RouterLink>
      </nav>
      <div
        class="flex flex-col items-center gap-0.5 border-t px-1 py-2 text-[10px] leading-tight"
        :class="keyOk ? 'text-primary' : 'text-destructive'"
        :title="keyOk ? t('ai.connected') : t('ai.disconnected')"
      >
        <Bot class="h-3.5 w-3.5" />
        <span class="text-center">{{ keyOk ? t("ai.connected") : t("ai.disconnected") }}</span>
      </div>
    </aside>
    <main class="min-h-0 min-w-0 flex-1 overflow-hidden">
      <RouterView />
    </main>
  </div>
</template>
