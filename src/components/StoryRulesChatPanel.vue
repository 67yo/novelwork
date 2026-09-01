<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { ArrowUp, Loader2, X } from "@lucide/vue";
import { api } from "@/lib/api";
import { useI18n } from "@/i18n";
import { usePersistedChatModel } from "@/lib/chatModel";
import { applyStoryRulesPayload } from "@/lib/storyRulesGen";
import { plainTree } from "@/lib/plainTree";
import { Button } from "@/components/ui/button";

const props = defineProps<{
  open: boolean;
  novelId: string;
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

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    error.value = "";
    if (!messages.value.length) {
      messages.value = [
        { id: "welcome", role: "assistant", content: t("workspace.sr.chatWelcome") },
      ];
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
  messages.value.push({ id: `u-${Date.now()}`, role: "user", content: text });
}

function pushAssistant(text: string) {
  messages.value.push({ id: `a-${Date.now()}`, role: "assistant", content: text });
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
    const res = await api.generateStoryRulesChat(props.novelId, turns, model.value);
    pushAssistant(res.assistant);
    const tr = await api.getTree(props.novelId);
    const changed = applyStoryRulesPayload(tr, res.blocks, (key) =>
      t(key as Parameters<typeof t>[0]),
    );
    if (changed) {
      const saved = await api.saveTreeJson(props.novelId, JSON.stringify(plainTree(tr)));
      if (!saved?.nodes?.length) throw new Error(t("workspace.sr.chatSaveFailed"));
      emit("applied");
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    sending.value = false;
    await scrollBottom();
  }
}

function onSend() {
  void sendText(draft.value);
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    onSend();
  }
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 flex items-end justify-center bg-black/40 sm:items-center sm:p-4"
    @click.self="emit('close')"
  >
    <div class="flex h-[min(90vh,640px)] w-full max-w-lg flex-col rounded-t-lg border bg-background shadow-xl sm:rounded-lg">
      <div class="flex items-center justify-between border-b px-4 py-3">
        <h2 class="text-sm font-semibold">{{ t("workspace.sr.chatTitle") }}</h2>
        <Button variant="ghost" size="icon" class="h-8 w-8" @click="emit('close')">
          <X class="h-4 w-4" />
        </Button>
      </div>
      <div ref="listEl" class="min-h-0 flex-1 overflow-y-auto p-4 space-y-3 text-sm">
        <div
          v-for="m in messages"
          :key="m.id"
          :class="m.role === 'user' ? 'text-right' : 'text-left'"
        >
          <div
            :class="[
              'inline-block max-w-[90%] rounded-lg px-3 py-2 text-sm',
              m.role === 'user' ? 'bg-primary text-primary-foreground' : 'bg-muted',
            ]"
          >
            {{ m.content }}
          </div>
        </div>
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
      </div>
      <div class="border-t p-3">
        <div class="flex gap-2">
          <textarea
            v-model="draft"
            rows="2"
            class="min-h-[2.5rem] flex-1 resize-none rounded-md border bg-background px-3 py-2 text-sm"
            :placeholder="t('workspace.sr.chatInputPh')"
            :disabled="sending"
            @keydown="onKeydown"
          />
          <Button size="icon" class="shrink-0" :disabled="sending || !draft.trim()" @click="onSend">
            <Loader2 v-if="sending" class="h-4 w-4 animate-spin" />
            <ArrowUp v-else class="h-4 w-4" />
          </Button>
        </div>
        <select
          v-model="model"
          class="mt-2 h-8 w-full rounded border bg-background px-2 text-xs"
          @focus="() => loadModel(t('settings.deprecated'))"
          @change="persistModel()"
        >
          <option v-for="o in options" :key="o.id" :value="o.id">{{ o.label }}</option>
        </select>
      </div>
    </div>
  </div>
</template>
