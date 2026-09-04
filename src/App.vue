<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
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
  List,
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
const openNovelTitle = ref("");
const keyOk = ref(false);
const mcpOk = ref(false);
const mcpEndpoint = ref("");
const lastNovelId = useLocalStorage("novework.lastNovelId", "");
const novelsTo = computed(() =>
  lastNovelId.value ? `/novels/${lastNovelId.value}` : "/novels",
);
const chatOpen = useLocalStorage("novework.chatOpen", false);
const FAB = 48;
const fabPos = useLocalStorage("novework.chatFab", { x: -1, y: -1 });
const chatMode = useLocalStorage<"dock" | "float">("novework.chatMode", "float");
const MIN_CHAT_W = 260;
const MIN_CHAT_H = 280;
const MIN_MAIN_W = 280;
const NAV_W = 64;
const chatW = useLocalStorage("novework.chatW", 352);
const floatRect = useLocalStorage("novework.chatFloat", {
  x: -1,
  y: 48,
  w: 400,
  h: 560,
});
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

function applyWindowTitle(name: string) {
  openNovelTitle.value = name;
  const title = name ? `${BASE_TITLE} - ${name}` : BASE_TITLE;
  document.title = title;
  void getCurrentWindow()
    .setTitle(title)
    .catch(() => {
      /* 非 Tauri 环境忽略 */
    });
}

async function syncWindowTitle() {
  const routeId = route.params.id;
  const id =
    route.name === "workspace" && typeof routeId === "string" && routeId
      ? routeId
      : route.name === "novels"
        ? ""
        : lastNovelId.value;
  if (!id) {
    applyWindowTitle("");
    return;
  }
  try {
    const novel = await api.getNovel(id);
    applyWindowTitle(novel?.title?.trim() ?? "");
  } catch {
    applyWindowTitle("");
  }
}

onMounted(() => {
  clampFloat();
  clampFab();
  window.addEventListener("resize", onWinResize);
  const bootId = route.params.id;
  if (route.name === "workspace" && typeof bootId === "string" && bootId) {
    lastNovelId.value = bootId;
  }
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
  window.removeEventListener("resize", onWinResize);
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
  ([name, id]) => {
    if (name === "workspace" && typeof id === "string" && id) lastNovelId.value = id;
    void syncWindowTitle();
  },
);

function navOn(active: boolean) {
  return [
    "flex w-full flex-col items-center gap-0.5 rounded-md px-1 py-2 text-[11px] leading-tight transition-colors",
    active ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
  ];
}

function navClass(path: string) {
  return navOn(route.path === path || route.path.startsWith(`${path}/`));
}

function onWinResize() {
  clampFloat();
  clampFab();
}

function clampFab() {
  const p = fabPos.value ?? { x: -1, y: -1 };
  let x = Number(p.x);
  let y = Number(p.y);
  if (!Number.isFinite(x) || x < 0) x = window.innerWidth - FAB - 16;
  if (!Number.isFinite(y) || y < 0) y = window.innerHeight - FAB - 16;
  x = Math.min(Math.max(8, x), Math.max(8, window.innerWidth - FAB - 8));
  y = Math.min(Math.max(8, y), Math.max(8, window.innerHeight - FAB - 8));
  if (p.x !== x || p.y !== y) fabPos.value = { x, y };
}

let fabSkipClick = false;

function startFab(ev: MouseEvent) {
  if (ev.button !== 0) return;
  ev.preventDefault();
  const sx = ev.clientX;
  const sy = ev.clientY;
  const ox = fabPos.value.x;
  const oy = fabPos.value.y;
  let dragged = false;
  const onMove = (e: MouseEvent) => {
    if (Math.abs(e.clientX - sx) + Math.abs(e.clientY - sy) > 4) dragged = true;
    if (!dragged) return;
    fabPos.value = { x: ox + e.clientX - sx, y: oy + e.clientY - sy };
    clampFab();
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    if (dragged) fabSkipClick = true;
    else chatOpen.value = true;
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function onFabClick() {
  if (fabSkipClick) {
    fabSkipClick = false;
    return;
  }
  chatOpen.value = true;
}

function clampFloat() {
  const r = floatRect.value ?? { x: -1, y: 48, w: 400, h: 560 };
  const maxW = Math.max(MIN_CHAT_W, window.innerWidth - 16);
  const maxH = Math.max(MIN_CHAT_H, window.innerHeight - 16);
  const w = Math.min(Math.max(Number(r.w) || 400, MIN_CHAT_W), maxW);
  const h = Math.min(Math.max(Number(r.h) || 560, MIN_CHAT_H), maxH);
  let x = Number(r.x);
  let y = Number(r.y);
  if (!Number.isFinite(x) || x < 0) x = Math.max(8, window.innerWidth - w - 16);
  if (!Number.isFinite(y) || y < 0) y = 48;
  x = Math.min(Math.max(8, x), Math.max(8, window.innerWidth - 80));
  y = Math.min(Math.max(8, y), Math.max(8, window.innerHeight - 48));
  if (r.x !== x || r.y !== y || r.w !== w || r.h !== h) {
    floatRect.value = { x, y, w, h };
  }
}

function toFloat() {
  if (floatRect.value.x < 0) {
    const w = chatW.value;
    const h = Math.round(Math.min(window.innerHeight * 0.72, window.innerHeight - 56));
    floatRect.value = {
      x: Math.max(8, window.innerWidth - w - 16),
      y: 48,
      w,
      h,
    };
  }
  chatMode.value = "float";
  clampFloat();
}

function toDock() {
  chatW.value = Math.max(MIN_CHAT_W, floatRect.value.w || chatW.value);
  chatMode.value = "dock";
}

function toggleChatMode() {
  if (chatMode.value === "float") toDock();
  else toFloat();
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

function startMoveFloat(ev: MouseEvent) {
  ev.preventDefault();
  const sx = ev.clientX;
  const sy = ev.clientY;
  const ox = floatRect.value.x;
  const oy = floatRect.value.y;
  const onMove = (e: MouseEvent) => {
    floatRect.value = {
      ...floatRect.value,
      x: ox + e.clientX - sx,
      y: oy + e.clientY - sy,
    };
    clampFloat();
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function startResizeFloat(ev: MouseEvent) {
  ev.preventDefault();
  ev.stopPropagation();
  const sx = ev.clientX;
  const sy = ev.clientY;
  const ow = floatRect.value.w;
  const oh = floatRect.value.h;
  const onMove = (e: MouseEvent) => {
    floatRect.value = {
      ...floatRect.value,
      w: ow + e.clientX - sx,
      h: oh + e.clientY - sy,
    };
    clampFloat();
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
        :title="openNovelTitle ? `${BASE_TITLE} - ${openNovelTitle}` : BASE_TITLE"
      >
        <Sparkles class="h-5 w-5" />
        <span class="text-[10px] font-semibold leading-tight tracking-tight">Novel</span>
      </RouterLink>
      <nav class="flex flex-1 flex-col gap-1 p-1.5">
        <RouterLink :to="novelsTo" :class="navOn(route.name === 'workspace')" :title="t('nav.novels')">
          <BookOpen class="h-4 w-4" />
          <span>{{ t("nav.novels") }}</span>
        </RouterLink>
        <RouterLink :to="'/novels'" :class="navOn(route.name === 'novels')" :title="t('nav.novelsListTitle')">
          <List class="h-4 w-4" />
          <span>{{ t("nav.novelsList") }}</span>
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
    <Teleport to="body">
      <button
        v-if="!chatOpen"
        type="button"
        class="fixed z-[80] flex cursor-grab items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg hover:opacity-90 active:cursor-grabbing"
        :style="{
          left: `${fabPos.x}px`,
          top: `${fabPos.y}px`,
          width: `${FAB}px`,
          height: `${FAB}px`,
        }"
        :title="t('globalChat.open')"
        :aria-label="t('globalChat.open')"
        @mousedown="startFab"
        @click="onFabClick"
      >
        <MessageSquare class="h-5 w-5" />
      </button>
    </Teleport>
    <Teleport to="body" :disabled="chatMode !== 'float'">
    <div
      v-if="chatOpen"
      class="flex"
      :class="
        chatMode === 'float'
          ? 'chat-float fixed z-[80] flex-col overflow-hidden rounded-lg border bg-background shadow-xl'
          : 'h-full shrink-0'
      "
      :style="
        chatMode === 'float'
          ? {
              left: `${floatRect.x}px`,
              top: `${floatRect.y}px`,
              width: `${floatRect.w}px`,
              height: `${floatRect.h}px`,
            }
          : { width: `${chatW}px` }
      "
    >
      <div
        v-if="chatMode === 'dock'"
        class="w-1 shrink-0 cursor-col-resize bg-border hover:bg-primary/40"
        :title="t('workspace.resize')"
        @mousedown="startResizeChat"
      />
      <GlobalChatPanel
        class="min-h-0 min-w-0 flex-1"
        :mode="chatMode"
        @close="chatOpen = false"
        @toggle-mode="toggleChatMode"
        @move="startMoveFloat"
      />
      <div
        v-if="chatMode === 'float'"
        class="absolute bottom-0 right-0 h-3.5 w-3.5 cursor-nwse-resize"
        :title="t('workspace.resize')"
        @mousedown="startResizeFloat"
      />
    </div>
    </Teleport>
  </div>
</template>
