<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { ArrowUp, Loader2, Square, X } from "@lucide/vue";
import { api } from "@/lib/api";
import { useI18n } from "@/i18n";
import { usePersistedChatModel } from "@/lib/chatModel";
import { applyWorldviewPayload } from "@/lib/worldviewGen";
import { plainTree } from "@/lib/plainTree";
import { applyAutoLayout } from "@/lib/treeLayout";
import { worldviewFanTitleKey, worldviewJsonKeyForSlot } from "@/lib/worldview";
import { Button } from "@/components/ui/button";

const props = defineProps<{
  open: boolean;
  novelId: string;
  slot?: string | null;
}>();

const emit = defineEmits<{
  close: [];
  applied: [];
}>();

type Msg = { id: string; role: "user" | "assistant"; content: string };

const { t } = useI18n();
const { model, options, load: loadModel, persist: persistModel } = usePersistedChatModel("chat_model");

const messages = ref<Msg[]>([]);
const draft = ref("");
const sending = ref(false);
const error = ref("");
const listEl = ref<HTMLElement | null>(null);
const expanded = ref(false);
const sessionSlot = ref<string | null | undefined>(undefined);

const slotKey = computed(() => worldviewJsonKeyForSlot(props.slot));
const slotTitle = computed(() => {
  const key = worldviewFanTitleKey(props.slot);
  return key ? t(key) : "";
});

function welcomeText() {
  return slotKey.value
    ? t("workspace.wv.chatSlotWelcome", { title: slotTitle.value })
    : t("workspace.wv.chatWelcome");
}

watch(
  () => [props.open, props.slot] as const,
  ([open]) => {
    if (!open) return;
    error.value = "";
    const s = props.slot ?? null;
    if (sessionSlot.value !== s || !messages.value.length) {
      messages.value = [
        {
          id: "welcome",
          role: "assistant",
          content: welcomeText(),
        },
      ];
      sessionSlot.value = s;
    }
    void loadModel(t("settings.deprecated"));
    void nextTick(() => scrollBottom());
  },
);

async function scrollBottom() {
  await nextTick();
  const el = listEl.value;
  if (el) el.scrollTop = el.scrollHeight;
}

function pushUser(text: string) {
  messages.value.push({
    id: `u-${Date.now()}`,
    role: "user",
    content: text,
  });
}

function pushAssistant(text: string) {
  messages.value.push({
    id: `a-${Date.now()}`,
    role: "assistant",
    content: text,
  });
}

async function sendText(text: string) {
  const trimmed = text.trim();
  if (!trimmed || sending.value) return;
  pushUser(trimmed);
  draft.value = "";
  sending.value = true;
  error.value = "";
  await scrollBottom();
  try {
    const turns = messages.value
      .filter((m) => m.id !== "welcome")
      .map((m) => ({ role: m.role, content: m.content }));
    const res = await api.generateWorldviewChat(
      props.novelId,
      turns,
      model.value,
      slotKey.value,
    );
    pushAssistant(res.assistant);
    const tr = await api.getTree(props.novelId);
    const changed = applyWorldviewPayload(tr, res.worldview, (key) =>
      t(key as Parameters<typeof t>[0]),
    );
    if (changed) {
      applyAutoLayout(tr.nodes, tr.edges);
      const saved = await api.saveTreeJson(props.novelId, JSON.stringify(plainTree(tr)));
      if (!saved?.nodes?.length) {
        throw new Error(t("workspace.wv.chatSaveFailed"));
      }
      emit("applied");
    }
    await scrollBottom();
  } catch (e) {
    const msg = String(e);
    if (msg.includes("cancelled")) {
      error.value = t("workspace.chatStopped");
    } else {
      error.value = msg;
    }
  } finally {
    sending.value = false;
  }
}

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  await sendText(text);
}

function sendPreset(key: "random" | "expand") {
  const title = slotTitle.value;
  const msg = slotKey.value
    ? t(key === "random" ? "workspace.wv.chatSlotRandomMsg" : "workspace.wv.chatSlotExpandMsg", {
        title,
      })
    : t(key === "random" ? "workspace.wv.chatRandomMsg" : "workspace.wv.chatExpandMsg");
  void sendText(msg);
}

async function stop() {
  try {
    await api.chatCancel(props.novelId);
  } catch {
    /* ignore */
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key !== "Enter" || e.isComposing) return;
  if (e.shiftKey) {
    expanded.value = true;
    return;
  }
  e.preventDefault();
  void send();
}

function close() {
  if (sending.value) return;
  emit("close");
}

const canSend = computed(() => !!draft.value.trim() && !sending.value);
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-[70] flex items-center justify-center bg-black/45 p-4"
    @click.self="close"
  >
    <div
      class="flex h-[min(640px,90vh)] w-full max-w-lg flex-col overflow-hidden rounded-lg border bg-background shadow-xl"
      role="dialog"
      aria-modal="true"
      :aria-label="slotKey ? t('workspace.wv.chatSlotTitle', { title: slotTitle }) : t('workspace.wv.chatTitle')"
    >
      <header class="flex shrink-0 items-center gap-2 border-b px-4 py-3">
        <h2 class="min-w-0 flex-1 text-sm font-semibold">{{
          slotKey ? t("workspace.wv.chatSlotTitle", { title: slotTitle }) : t("workspace.wv.chatTitle")
        }}</h2>
        <button
          type="button"
          class="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          :disabled="sending"
          :aria-label="t('novels.cancel')"
          @click="close"
        >
          <X class="h-4 w-4" />
        </button>
      </header>

      <p class="shrink-0 border-b px-4 py-2 text-[11px] text-muted-foreground">
        {{
          slotKey
            ? t("workspace.wv.chatSlotHint", { title: slotTitle })
            : t("workspace.wv.chatHint")
        }}
      </p>

      <div class="flex shrink-0 flex-wrap gap-1.5 px-4 py-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 text-xs"
          :disabled="sending"
          @click="sendPreset('random')"
        >
          {{ t("workspace.wv.chatRandom") }}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          class="h-7 text-xs"
          :disabled="sending"
          @click="sendPreset('expand')"
        >
          {{ t("workspace.wv.chatExpand") }}
        </Button>
      </div>

      <div
        ref="listEl"
        class="min-h-0 flex-1 space-y-2 overflow-y-auto px-4 py-2"
      >
        <div
          v-for="m in messages"
          :key="m.id"
          class="rounded-lg px-3 py-2 text-sm leading-relaxed"
          :class="
            m.role === 'user'
              ? 'ml-8 bg-primary text-primary-foreground'
              : 'mr-4 bg-muted text-foreground'
          "
        >
          <div class="whitespace-pre-wrap break-words">{{ m.content }}</div>
        </div>
        <div v-if="sending" class="mr-4 flex items-center gap-2 rounded-lg bg-muted px-3 py-2 text-xs text-muted-foreground">
          <Loader2 class="h-3.5 w-3.5 animate-spin" />
          {{
            slotKey
              ? t("workspace.wv.chatSlotBusy", { title: slotTitle })
              : t("workspace.wv.chatBusy")
          }}
        </div>
        <p v-if="error" class="rounded-md bg-destructive/10 px-2 py-1.5 text-xs text-destructive">
          {{ error }}
        </p>
      </div>

      <footer class="shrink-0 border-t p-3">
        <div class="rounded-xl border bg-muted/30 px-3 py-2">
          <textarea
            v-model="draft"
            :rows="expanded ? 4 : 2"
            class="w-full resize-none bg-transparent text-sm leading-6 outline-none placeholder:text-muted-foreground disabled:opacity-50"
            :placeholder="t('workspace.wv.chatPh')"
            :disabled="sending"
            @keydown="onKeydown"
          />
          <div class="mt-1 flex items-center gap-1">
            <select
              v-model="model"
              class="h-7 max-w-[10rem] truncate rounded-md border-0 bg-transparent px-1 text-xs text-muted-foreground outline-none"
              :disabled="sending"
              @focus="() => loadModel(t('settings.deprecated'))"
              @change="persistModel"
            >
              <option v-for="m in options" :key="m.id" :value="m.id">{{ m.label }}</option>
            </select>
            <div class="flex-1" />
            <button
              v-if="!sending"
              type="button"
              class="inline-flex h-7 w-7 items-center justify-center rounded-full bg-primary text-primary-foreground disabled:opacity-40"
              :disabled="!canSend"
              :aria-label="t('globalChat.send')"
              @click="send"
            >
              <ArrowUp class="h-3.5 w-3.5" />
            </button>
            <button
              v-else
              type="button"
              class="inline-flex h-7 w-7 items-center justify-center rounded-full border hover:bg-muted"
              :aria-label="t('globalChat.stop')"
              @click="stop"
            >
              <Square class="h-3.5 w-3.5 fill-current" />
            </button>
          </div>
        </div>
      </footer>
    </div>
  </div>
</template>
