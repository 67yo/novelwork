<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RouterLink, RouterView, useRoute } from "vue-router";
import { useLocalStorage } from "@vueuse/core";
import { api } from "@/lib/api";
import { setLocalePreference, useI18n, type LocalePreference } from "@/i18n";
import {
  BookOpen,
  Bot,
  ChartColumnStacked,
  Library,
  MessageSquare,
  Plug,
  Settings,
  Sparkles,
} from "@lucide/vue";
import GlobalChatPanel from "@/components/GlobalChatPanel.vue";
import { subscribeGlobalChatOpen } from "@/lib/globalChatBridge";
import { useUiTheme } from "@/lib/uiTheme";

const BASE_TITLE = "Novel Work";
const route = useRoute();
const keyOk = ref(false);
const mcpOk = ref(false);
const mcpEndpoint = ref("");
const chatOpen = useLocalStorage("novework.chatOpen", true);
const MIN_CHAT_W = 260;
const MIN_MAIN_W = 280;
const NAV_W = 64;
const chatW = ref(352);
const { t } = useI18n();
useUiTheme();
let mcpTimer: number | undefined;
let unsubChatOpen: (() => void) | null = null;

async function refreshKey() {
  try {
    const s = await api.getSettings();
    keyOk.value = (s.compat_providers || []).some((p) => p.api_key_configured);
    setLocalePreference((s.ui_locale || "system") as LocalePreference);
  } catch {
    keyOk.value = false;
  }
}

async function refreshMcp() {
  try {
    const s = await api.getMcpStatus();
    mcpOk.value = !!s.running;
    mcpEndpoint.value = s.error ? `${s.endpoint} · ${s.error}` : s.endpoint || "";
  } catch {
    mcpOk.value = false;
    mcpEndpoint.value = "";
  }
}

async function syncWindowTitle() {
  try {
    const id = route.params.id;
    if (route.name === "workspace" && typeof id === "string" && id) {
      const novel = await api.getNovel(id);
      const name = novel?.title?.trim();
      await getCurrentWindow().setTitle(name ? `${BASE_TITLE} - ${name}` : BASE_TITLE);
    } else {
      await getCurrentWindow().setTitle(BASE_TITLE);
    }
  } catch {
    // 非 Tauri 环境忽略
  }
}

onMounted(() => {
  void refreshKey();
  void refreshMcp();
  void syncWindowTitle();
  mcpTimer = window.setInterval(() => void refreshMcp(), 4000);
  unsubChatOpen = subscribeGlobalChatOpen(() => {
    chatOpen.value = true;
  });
});
watch(
  () => route.path,
  () => {
    void refreshKey();
  },
);
onUnmounted(() => {
  if (mcpTimer) window.clearInterval(mcpTimer);
  unsubChatOpen?.();
  unsubChatOpen = null;
});
watch(() => route.path, () => {
  void refreshKey();
  void refreshMcp();
});
watch(
  () => [route.name, route.params.id] as const,
  () => {
    void syncWindowTitle();
  },
);

function navClass(path: string) {
  const active =
    path === "/novels" ? route.path.startsWith("/novels") : route.path === path || route.path.startsWith(`${path}/`);
  return [
    "flex w-full flex-col items-center gap-0.5 rounded-md px-1 py-2 text-[11px] leading-tight transition-colors",
    active ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
  ];
}

function chatBtnClass() {
  return [
    "flex w-full flex-col items-center gap-0.5 rounded-md px-1 py-2 text-[11px] leading-tight transition-colors",
    chatOpen.value
      ? "bg-accent text-accent-foreground"
      : "text-muted-foreground hover:bg-muted hover:text-foreground",
  ];
}

function startResizeChat(ev: MouseEvent) {
  ev.preventDefault();
  const startX = ev.clientX;
  const startW = chatW.value;
  const onMove = (e: MouseEvent) => {
    const max = Math.max(MIN_CHAT_W, window.innerWidth - NAV_W - MIN_MAIN_W);
    chatW.value = Math.min(Math.max(startW - (e.clientX - startX), MIN_CHAT_W), max);
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}
</script>

<template>
  <div class="flex h-screen overflow-hidden">
    <aside
      class="app-nav z-50 flex w-16 shrink-0 flex-col border-r"
    >
      <RouterLink
        to="/novels"
        class="flex flex-col items-center gap-0.5 border-b px-1 py-3 text-primary"
        :title="'Novel Work'"
      >
        <Sparkles class="h-5 w-5" />
        <span class="text-[10px] font-semibold leading-tight tracking-tight">Novel</span>
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
        <button type="button" :class="chatBtnClass()" :title="t('nav.chat')" @click="chatOpen = !chatOpen">
          <MessageSquare class="h-4 w-4" />
          <span>{{ t("nav.chat") }}</span>
        </button>
        <RouterLink :to="'/settings'" :class="navClass('/settings')" :title="t('nav.settings')">
          <Settings class="h-4 w-4" />
          <span>{{ t("nav.settings") }}</span>
        </RouterLink>
      </nav>
      <div class="flex flex-col gap-1 border-t px-1 py-2">
        <div
          class="flex flex-col items-center gap-0.5 text-[10px] leading-tight"
          :class="keyOk ? 'text-primary' : 'text-destructive'"
          :title="keyOk ? t('ai.connected') : t('ai.disconnected')"
        >
          <Bot class="h-3.5 w-3.5" />
          <span class="text-center">{{ keyOk ? t("ai.connected") : t("ai.disconnected") }}</span>
        </div>
        <div
          class="flex flex-col items-center gap-0.5 text-[10px] leading-tight"
          :class="mcpOk ? 'text-primary' : 'text-muted-foreground'"
          :title="mcpEndpoint || (mcpOk ? t('mcp.running') : t('mcp.stopped'))"
        >
          <Plug class="h-3.5 w-3.5" />
          <span class="text-center">{{ mcpOk ? t("mcp.running") : t("mcp.stopped") }}</span>
        </div>
      </div>
    </aside>
    <main class="min-h-0 min-w-0 flex-1 overflow-hidden">
      <RouterView />
    </main>
    <div v-if="chatOpen" class="flex h-full shrink-0" :style="{ width: `${chatW}px` }">
      <div
        class="w-1 shrink-0 cursor-col-resize bg-border hover:bg-primary/40"
        :title="t('workspace.resize')"
        @mousedown="startResizeChat"
      />
      <GlobalChatPanel class="min-w-0 flex-1" @close="chatOpen = false" />
    </div>
  </div>
</template>
