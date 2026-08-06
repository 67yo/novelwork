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
    "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
    active ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
  ];
}
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden">
    <header class="z-50 flex h-14 shrink-0 items-center justify-between border-b bg-[linear-gradient(120deg,oklch(0.97_0.02_155),oklch(0.99_0.01_85))] px-4">
      <div class="flex items-center gap-6">
        <RouterLink to="/novels" class="flex items-center gap-2">
          <Sparkles class="h-5 w-5 text-primary" />
          <span class="text-lg font-semibold tracking-tight">Nove Work</span>
        </RouterLink>
        <nav class="flex items-center gap-1">
          <RouterLink :to="'/novels'" :class="navClass('/novels')">
            <BookOpen class="h-4 w-4" /> {{ t("nav.novels") }}
          </RouterLink>
          <RouterLink :to="'/library'" :class="navClass('/library')">
            <Library class="h-4 w-4" /> {{ t("nav.library") }}
          </RouterLink>
          <RouterLink :to="'/stats'" :class="navClass('/stats')">
            <ChartColumnStacked class="h-4 w-4" /> {{ t("nav.stats") }}
          </RouterLink>
          <RouterLink :to="'/settings'" :class="navClass('/settings')">
            <Settings class="h-4 w-4" /> {{ t("nav.settings") }}
          </RouterLink>
        </nav>
      </div>
      <div class="flex items-center gap-1.5 text-xs text-muted-foreground">
        <Bot class="h-3.5 w-3.5" :class="keyOk ? 'text-primary' : 'text-destructive'" />
        <span :class="keyOk ? 'text-primary' : 'text-destructive'">
          {{ keyOk ? t("ai.connected") : t("ai.disconnected") }}
        </span>
      </div>
    </header>
    <main class="min-h-0 flex-1 overflow-auto">
      <RouterView />
    </main>
  </div>
</template>
