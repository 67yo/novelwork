<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowUp,
  Maximize2,
  MessageSquare,
  Minimize2,
  PanelRightClose,
  Square,
  Trash2,
} from "@lucide/vue";
import { api, type GlobalChatMessage, type SkillPreviewItem } from "@/lib/api";
import { useI18n } from "@/i18n";
import { usePersistedChatModel } from "@/lib/chatModel";
import { choiceReply, joinChosen, parseChatChoices, type ChatChoices } from "@/lib/chatChoices";
import { createChatRunProgress, type ChatRunProgress } from "@/lib/chatRunProgress";
import { subscribeGlobalChatSend } from "@/lib/globalChatBridge";

const emit = defineEmits<{ close: [] }>();

const { t } = useI18n();
const route = useRoute();
const messages = ref<GlobalChatMessage[]>([]);
const draft = ref("");
const sending = ref(false);
const error = ref("");
const expanded = ref(false);
const boundTitle = ref("");
const listEl = ref<HTMLElement | null>(null);
const chatSkills = ref<SkillPreviewItem[]>([]);
const slashActive = ref(0);
const slashHide = ref(false);
const pendingLines = ref<string[]>([]);
const lastUserMsgId = ref("");
const lastUserTokens = ref<{ n: number; confirmed: boolean } | null>(null);
const inflightText = ref("");
const aborting = ref(false);
const { model, options, load: loadModel, persist: persistModel } = usePersistedChatModel("chat_model");

let run: ChatRunProgress | null = null;
let unlistenProgress: UnlistenFn | null = null;
let unlistenTokens: UnlistenFn | null = null;
let unsubBridge: (() => void) | null = null;

function formatStepMs(ms: number): string {
  const sec = Math.max(0, ms) / 1000;
  if (sec < 60) return `${sec.toFixed(1)}s`;
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function stepLabel(step: string): string {
  if (step.startsWith("tool:")) return t("globalChat.stepTool", { name: step.slice(5) });
  if (step === "connecting") return t("globalChat.stepConnecting");
  if (step === "thinking") return t("globalChat.stepThinking");
  if (step === "writing") return t("globalChat.stepWriting");
  return step;
}

function applyTokens(prompt: number, completion: number, confirmed: boolean) {
  run?.onTokens(prompt, completion, confirmed);
  if (prompt <= 0) return;
  if (confirmed) {
    lastUserTokens.value = {
      n: lastUserTokens.value?.confirmed ? lastUserTokens.value.n + prompt : prompt,
      confirmed: true,
    };
  } else if (!lastUserTokens.value?.confirmed) {
    lastUserTokens.value = { n: prompt, confirmed: false };
  }
}

function stopRun() {
  run?.dispose();
  run = null;
  pendingLines.value = [];
}

type SlashCmd = { cmd: string; insert: string; hint: string };

const slashSuggestions = computed(() => {
  const v = draft.value;
  if (slashHide.value || !v) return [];
  const prefix = v.startsWith("/") ? "/" : v.startsWith("@") ? "@" : "";
  if (!prefix) return [];
  const q = v.toLowerCase();
  const token = v.slice(prefix.length).trim().split(/\s+/).pop()?.toLowerCase() ?? "";
  const out: SlashCmd[] = [];
  for (const s of chatSkills.value) {
    const skillCmd: string = `${prefix}${s.name}`;
    const skillHint =
      (s.description || "").split(/\n/)[0]?.trim() || t("workspace.slashHintSkill");
    if (skillCmd.toLowerCase().startsWith(q) || q === prefix) {
      out.push({
        cmd: skillCmd,
        insert: `${skillCmd} `,
        hint: skillHint.length > 72 ? `${skillHint.slice(0, 72)}…` : skillHint,
      });
    }
    for (const h of s.slash_hints ?? []) {
      const full = `${skillCmd} ${h.cmd}`;
      const hit =
        full.toLowerCase().startsWith(q) ||
        (token.length > 0 && (h.cmd.startsWith(token) || full.toLowerCase().includes(token)));
      if (!hit) continue;
      out.push({
        cmd: full,
        insert: `${full} `,
        hint: h.hint,
      });
    }
  }
  return out;
});

watch(slashSuggestions, () => {
  slashActive.value = 0;
});

watch(draft, () => {
  slashHide.value = false;
});

function applySlashCmd(cmd: SlashCmd) {
  draft.value = cmd.insert;
  slashActive.value = 0;
}

const novelId = computed(() =>
  route.name === "workspace" && typeof route.params.id === "string" && route.params.id
    ? route.params.id
    : null,
);

const headerTitle = computed(() => boundTitle.value.trim() || t("globalChat.title"));

async function refresh() {
  try {
    messages.value = await api.globalChatList(novelId.value);
  } catch {
    messages.value = [];
  }
}

async function scrollBottom() {
  await nextTick();
  const el = listEl.value;
  if (el) el.scrollTop = el.scrollHeight;
}

async function sendText(text: string) {
  const trimmed = text.trim();
  if (!trimmed || sending.value) return;
  sending.value = true;
  aborting.value = false;
  error.value = "";
  const optimistic: GlobalChatMessage = {
    id: `local-${Date.now()}`,
    role: "user",
    content: trimmed,
    created_at: new Date().toISOString(),
  };
  lastUserMsgId.value = optimistic.id;
  inflightText.value = trimmed;
  lastUserTokens.value = { n: Math.max(1, Math.floor(trimmed.length / 4)), confirmed: false };
  messages.value = [...messages.value, optimistic];
  stopRun();
  run = createChatRunProgress({
    initialLabel: t("globalChat.stepConnecting"),
    formatMs: formatStepMs,
    formatPrompt: (n, confirmed) =>
      t(confirmed ? "workspace.taskPromptTokens" : "workspace.taskPromptTokensEst", { n }),
    formatCompletion: (n) => t("workspace.taskCompletionTokens", { n }),
    formatTotal: (time) => t("workspace.taskTotalTime", { t: time }),
    onLines: (lines) => {
      pendingLines.value = lines;
      void scrollBottom();
    },
  });
  await scrollBottom();
  try {
    const next = await api.globalChatSend({
      content: trimmed,
      routeName: typeof route.name === "string" ? route.name : String(route.name ?? ""),
      path: route.fullPath,
      novelId: novelId.value,
      model: model.value,
    });
    messages.value = next;
    const lastUser = [...next].reverse().find((m) => m.role === "user");
    lastUserMsgId.value = lastUser?.id ?? "";
    if (!lastUser || lastUser.content !== trimmed) lastUserTokens.value = null;
    await scrollBottom();
  } catch (e) {
    if (!aborting.value) {
      error.value = e instanceof Error ? e.message : String(e);
    }
    await refresh();
  } finally {
    inflightText.value = "";
    stopRun();
    sending.value = false;
  }
}

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  draft.value = "";
  await sendText(text);
}

const lastChoices = computed((): ChatChoices | null => {
  const last = messages.value.at(-1);
  if (!last || last.role !== "assistant" || sending.value) return null;
  return parseChatChoices(last.content);
});

const lastChoiceLabels = computed(() => {
  const c = lastChoices.value;
  if (!c) return [];
  if (c.kind === "yn") return [t("globalChat.choiceYes"), t("globalChat.choiceNo")];
  return c.options;
});

const multiPicked = ref<string[]>([]);

watch(
  () => messages.value.at(-1)?.id,
  () => {
    multiPicked.value = [];
  },
);

function toggleMulti(label: string) {
  const i = multiPicked.value.indexOf(label);
  if (i >= 0) multiPicked.value.splice(i, 1);
  else multiPicked.value.push(label);
}

function sendChoice(label: string) {
  const last = messages.value.at(-1);
  const c = lastChoices.value;
  if (last?.role === "assistant" && c) {
    void sendText(choiceReply(last.content, label, c));
    return;
  }
  void sendText(label);
}

function onComposerKeydown(e: KeyboardEvent) {
  const list = slashSuggestions.value;
  if (list.length) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      slashActive.value = (slashActive.value + 1) % list.length;
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      slashActive.value = (slashActive.value - 1 + list.length) % list.length;
      return;
    }
    if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      applySlashCmd(list[slashActive.value] ?? list[0]);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      slashHide.value = true;
      return;
    }
  }
  if (e.key !== "Enter" || e.isComposing) return;
  if (e.shiftKey) {
    if (!expanded.value) expanded.value = true;
    return;
  }
  e.preventDefault();
  void send();
}

async function stop() {
  aborting.value = true;
  const text = inflightText.value;
  if (text) draft.value = text;
  const last = messages.value.at(-1);
  if (last?.role === "user" && last.content === text) {
    messages.value = messages.value.slice(0, -1);
    const prev = [...messages.value].reverse().find((m) => m.role === "user");
    lastUserMsgId.value = prev?.id ?? "";
    lastUserTokens.value = null;
  }
  try {
    await api.globalChatCancel();
  } catch {
    /* ignore */
  }
}

async function clear() {
  try {
    await api.globalChatClear(novelId.value);
    messages.value = [];
    error.value = "";
    lastUserMsgId.value = "";
    lastUserTokens.value = null;
    stopRun();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

watch(
  novelId,
  async (id) => {
    boundTitle.value = "";
    lastUserMsgId.value = "";
    lastUserTokens.value = null;
    if (id) {
      try {
        const n = await api.getNovel(id);
        boundTitle.value = n?.title?.trim() ?? "";
      } catch {
        boundTitle.value = "";
      }
    }
    await refresh();
    await scrollBottom();
  },
  { immediate: true },
);

onMounted(() => {
  unsubBridge = subscribeGlobalChatSend((text) => {
    void sendText(text);
  });
  void loadModel(t("settings.deprecated"));
  void api
    .listChatSkills()
    .then((p) => {
      chatSkills.value = p.skills;
    })
    .catch(() => {
      chatSkills.value = [];
    });
  void listen<{ step: string }>("global-chat-progress", (ev) => {
    const step = ev.payload?.step?.trim();
    if (!step || !run) return;
    run.advance(step, stepLabel(step));
  }).then((fn) => {
    unlistenProgress = fn;
  }).catch(() => {});
  void listen<{
    promptTokens: number;
    completionTokens: number;
    confirmed: boolean;
  }>("global-chat-tokens", (ev) => {
    applyTokens(ev.payload.promptTokens, ev.payload.completionTokens, ev.payload.confirmed);
  }).then((fn) => {
    unlistenTokens = fn;
  }).catch(() => {});
});

watch(
  () => route.path,
  () => {
    void loadModel(t("settings.deprecated"));
  },
);

onUnmounted(() => {
  unsubBridge?.();
  unsubBridge = null;
  unlistenProgress?.();
  unlistenTokens?.();
  stopRun();
});
</script>

<template>
  <aside
    class="flex h-full min-w-0 w-full flex-col border-l bg-background"
    :aria-label="headerTitle"
  >
    <header class="flex min-w-0 shrink-0 items-center gap-2 border-b px-3 py-2.5">
      <MessageSquare class="h-4 w-4 shrink-0 text-primary" />
      <h2 class="min-w-0 flex-1 truncate text-sm font-semibold" :title="headerTitle">
        {{ headerTitle }}
      </h2>
      <button
        type="button"
        class="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
        :title="t('globalChat.clear')"
        :disabled="sending"
        @click="clear"
      >
        <Trash2 class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
        :title="t('globalChat.hide')"
        @click="emit('close')"
      >
        <PanelRightClose class="h-4 w-4" />
      </button>
    </header>

    <div
      ref="listEl"
      class="min-h-0 min-w-0 w-full flex-1 space-y-3 overflow-x-hidden overflow-y-auto px-3 py-3"
    >
      <p v-if="!messages.length" class="text-center text-sm text-muted-foreground">
        {{
          boundTitle
            ? t("globalChat.emptyBound", { title: boundTitle })
            : t("globalChat.empty")
        }}
      </p>
      <div
        v-for="(m, i) in messages"
        :key="m.id"
        class="min-w-0 max-w-full rounded-lg px-3 py-2 text-sm leading-relaxed"
        :class="
          m.role === 'user'
            ? 'ml-6 bg-primary text-primary-foreground'
            : 'mr-6 bg-muted text-foreground'
        "
      >
        <div class="min-w-0 max-w-full whitespace-pre-wrap break-normal [overflow-wrap:anywhere]">{{ m.content }}</div>
        <p
          v-if="m.id === lastUserMsgId && lastUserTokens"
          class="mt-1 text-[11px] leading-4 opacity-75"
        >
          {{
            t(
              lastUserTokens.confirmed
                ? "workspace.taskPromptTokens"
                : "workspace.taskPromptTokensEst",
              { n: lastUserTokens.n },
            )
          }}
        </p>
        <div
          v-if="i === messages.length - 1 && lastChoices && !sending"
          class="mt-2 flex flex-wrap gap-1.5"
        >
          <template v-if="lastChoices.multi">
            <label
              v-for="label in lastChoiceLabels"
              :key="label"
              class="flex min-h-8 cursor-pointer items-center gap-1.5 rounded-md border bg-background px-2 py-1 text-xs"
            >
              <input
                type="checkbox"
                class="h-3.5 w-3.5"
                :checked="multiPicked.includes(label)"
                @change="toggleMulti(label)"
              />
              {{ label }}
            </label>
            <button
              type="button"
              class="min-h-8 rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground disabled:opacity-40"
              :disabled="!multiPicked.length"
              @click="sendChoice(joinChosen(multiPicked))"
            >
              {{ t("globalChat.choiceConfirm") }}
            </button>
          </template>
          <button
            v-else
            v-for="label in lastChoiceLabels"
            :key="label"
            type="button"
            class="min-h-8 rounded-md border bg-background px-2.5 py-1 text-xs hover:bg-primary hover:text-primary-foreground"
            @click="sendChoice(label)"
          >
            {{ label }}
          </button>
        </div>
      </div>
      <div
        v-if="sending"
        class="mr-6 min-w-0 max-w-full rounded-lg bg-muted px-3 py-2 text-sm leading-relaxed text-foreground"
      >
        <p
          v-for="(line, li) in pendingLines"
          :key="li"
          class="text-xs leading-5 text-muted-foreground"
        >
          {{ line }}
        </p>
        <p v-if="!pendingLines.length" class="text-xs text-muted-foreground">
          {{ t("globalChat.sending") }}
        </p>
      </div>
      <p v-if="error" class="rounded-md bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
        {{ error }}
      </p>
    </div>

    <footer class="relative min-w-0 shrink-0 p-3 pt-1">
      <ul
        v-if="slashSuggestions.length"
        class="absolute inset-x-3 bottom-full z-20 mb-1 max-h-64 overflow-auto rounded-md border bg-popover py-1 text-sm shadow-md"
        role="listbox"
      >
        <li
          v-for="(s, i) in slashSuggestions"
          :key="s.cmd"
          class="cursor-pointer px-3 py-1.5"
          :class="i === slashActive ? 'bg-accent text-accent-foreground' : 'hover:bg-muted'"
          role="option"
          :aria-selected="i === slashActive"
          @mousedown.prevent="applySlashCmd(s)"
        >
          <span class="font-medium">{{ s.cmd }}</span>
          <span class="ml-2 text-xs text-muted-foreground">{{ s.hint }}</span>
        </li>
      </ul>
      <div
        class="rounded-2xl border border-border bg-muted/40 px-3 py-2 shadow-sm transition-colors focus-within:border-ring/60 focus-within:bg-background"
      >
        <textarea
          v-model="draft"
          :rows="expanded ? 8 : 1"
          class="w-full resize-none bg-transparent text-sm leading-6 outline-none placeholder:text-muted-foreground disabled:opacity-50"
          :class="expanded ? 'min-h-[10.5rem]' : 'min-h-6 max-h-6 overflow-hidden'"
          :placeholder="t('globalChat.placeholder')"
          :disabled="sending"
          @keydown="onComposerKeydown"
        />
        <div class="mt-1 flex items-center gap-1">
          <select
            v-model="model"
            class="h-7 max-w-[11rem] truncate rounded-md border-0 bg-transparent px-1 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground disabled:opacity-50"
            :title="t('chat.pickModel')"
            :disabled="sending"
            @focus="() => loadModel(t('settings.deprecated'))"
            @change="persistModel"
          >
            <option v-for="m in options" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
          <button
            type="button"
            class="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
            :title="expanded ? t('globalChat.collapse') : t('globalChat.expand')"
            @click="expanded = !expanded"
          >
            <Minimize2 v-if="expanded" class="h-3.5 w-3.5" />
            <Maximize2 v-else class="h-3.5 w-3.5" />
          </button>
          <div class="flex-1" />
          <button
            v-if="!sending"
            type="button"
            class="inline-flex h-7 w-7 items-center justify-center rounded-full bg-primary text-primary-foreground disabled:opacity-40"
            :disabled="!draft.trim()"
            :title="t('globalChat.sendHint')"
            :aria-label="t('globalChat.send')"
            @click="send"
          >
            <ArrowUp class="h-3.5 w-3.5" />
          </button>
          <button
            v-else
            type="button"
            class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-border text-foreground hover:bg-muted"
            :title="t('globalChat.stop')"
            :aria-label="t('globalChat.stop')"
            @click="stop"
          >
            <Square class="h-3 w-3 fill-current" />
          </button>
        </div>
      </div>
    </footer>
  </aside>
</template>
