<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api, type ChatTurn, type NovelProject } from "@/lib/api";
import { formatChatContent } from "@/lib/chatFormat";
import { usePersistedChatModel } from "@/lib/chatModel";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

function coverSrc(n: NovelProject) {
  return n.cover_path ? convertFileSrc(n.cover_path) : "";
}

const { t } = useI18n();
const router = useRouter();
const novels = ref<NovelProject[]>([]);
const tab = ref<"active" | "archived">("active");
const showCreate = ref(false);
const messages = ref<ChatTurn[]>([]);
const chatInput = ref("");
const busy = ref(false);
const error = ref("");
const listEl = ref<HTMLElement | null>(null);
const {
  model: chatModel,
  options: chatModelOptions,
  load: loadChatModel,
  persist: persistChatModel,
} = usePersistedChatModel("create_model");

const visible = computed(() =>
  novels.value.filter((n) => (tab.value === "archived" ? n.archived : !n.archived)),
);

function resetMessages() {
  messages.value = [{ role: "assistant", content: t("novels.createWelcome") }];
}

async function refresh() {
  novels.value = await api.listNovels();
}

function openCreate() {
  showCreate.value = true;
  error.value = "";
  resetMessages();
}

onMounted(() => {
  void refresh();
  void loadChatModel(t("settings.deprecated"));
});

function closeCreate() {
  showCreate.value = false;
  chatInput.value = "";
  error.value = "";
}

async function scrollBottom() {
  await nextTick();
  if (listEl.value) listEl.value.scrollTop = listEl.value.scrollHeight;
}

function onChatKeydown(ev: KeyboardEvent) {
  if (ev.key === "Enter" && !ev.shiftKey) {
    ev.preventDefault();
    void send(false);
  }
}

async function send(forceCreate = false) {
  const text = chatInput.value.trim();
  if (!forceCreate && !text) return;
  error.value = "";
  busy.value = true;
  try {
    const next = [...messages.value];
    if (text) {
      next.push({ role: "user", content: text });
      messages.value = next;
      chatInput.value = "";
      await scrollBottom();
    } else if (forceCreate) {
      next.push({ role: "user", content: t("novels.forceCreateMsg") });
      messages.value = next;
      await scrollBottom();
    }

    const r = await api.createNovelChat(messages.value, forceCreate, chatModel.value);
    messages.value = [...messages.value, { role: "assistant", content: r.reply }];
    await scrollBottom();
    if (r.used_mock) {
      error.value = t("novels.mockHint");
    }
    if (r.novel) {
      await router.push(`/novels/${r.novel.id}`);
    }
  } catch (e) {
    const msg = String(e);
    if (msg.toLowerCase().includes("cancelled")) {
      messages.value = [
        ...messages.value,
        { role: "assistant", content: t("workspace.chatStopped") },
      ];
    } else {
      error.value = msg;
    }
  } finally {
    busy.value = false;
  }
}

async function stopChat() {
  if (!busy.value) return;
  try {
    await api.chatCancel("__create__");
  } catch {
    /* ignore */
  }
}

async function archive(n: NovelProject, archived: boolean, ev: Event) {
  ev.stopPropagation();
  await api.archiveNovel(n.id, archived);
  await refresh();
}

const pendingDelete = ref<NovelProject | null>(null);
const deleting = ref(false);

function askDelete(n: NovelProject, ev: Event) {
  ev.stopPropagation();
  pendingDelete.value = n;
  error.value = "";
}

function cancelDelete() {
  if (deleting.value) return;
  pendingDelete.value = null;
}

async function confirmDelete() {
  const n = pendingDelete.value;
  if (!n) return;
  deleting.value = true;
  try {
    await api.deleteNovel(n.id);
    pendingDelete.value = null;
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    deleting.value = false;
  }
}
</script>

<template>
  <div class="mx-auto h-full max-w-5xl space-y-6 overflow-y-auto overscroll-contain p-6">
    <div class="flex items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("novels.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t("novels.subtitle") }}</p>
      </div>
      <Button @click="showCreate ? closeCreate() : openCreate()">
        {{ showCreate ? t("novels.cancel") : t("novels.create") }}
      </Button>
    </div>

    <div class="flex gap-2 text-sm">
      <button
        class="rounded-md px-3 py-1.5"
        :class="tab === 'active' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
        @click="tab = 'active'"
      >
        {{ t("novels.tabActive") }}
      </button>
      <button
        class="rounded-md px-3 py-1.5"
        :class="tab === 'archived' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
        @click="tab = 'archived'"
      >
        {{ t("novels.tabArchived") }}
      </button>
    </div>

    <Card v-if="showCreate" class="max-w-xl border-primary/20">
      <CardHeader class="pb-2">
        <CardTitle class="text-base">{{ t("novels.createChat") }}</CardTitle>
        <p class="text-xs text-muted-foreground">{{ t("novels.createHint") }}</p>
      </CardHeader>
      <CardContent class="space-y-3">
        <div ref="listEl" class="h-56 space-y-2 overflow-y-auto rounded-md border bg-muted/30 p-3">
          <div
            v-for="(m, i) in messages"
            :key="i"
            class="rounded-lg px-3 py-2 text-sm"
            :class="m.role === 'user' ? 'ml-8 bg-accent' : 'mr-6 bg-background'"
          >
            <div class="mb-0.5 text-[10px] uppercase text-muted-foreground">{{ m.role }}</div>
            <div class="whitespace-pre-wrap break-words leading-relaxed">
              {{ formatChatContent(m.content) }}
            </div>
          </div>
        </div>
        <Textarea
          v-model="chatInput"
          rows="2"
          :placeholder="t('novels.placeholder')"
          @keydown="onChatKeydown"
        />
        <div class="flex flex-wrap items-center gap-2">
          <select
            v-model="chatModel"
            class="h-9 min-w-[9rem] flex-1 rounded-md border border-input bg-background px-2 text-xs"
            :aria-label="t('chat.pickModel')"
            :disabled="busy"
            @change="persistChatModel"
          >
            <option v-for="m in chatModelOptions" :key="m.id" :value="m.id">{{ m.label }}</option>
          </select>
          <Button v-if="busy" variant="destructive" @click="stopChat">
            {{ t("workspace.stop") }}
          </Button>
          <template v-else>
            <Button @click="send(false)">
              {{ t("novels.send") }}
            </Button>
            <Button variant="secondary" @click="send(true)">
              {{ t("novels.forceCreate") }}
            </Button>
          </template>
        </div>
        <p v-if="error" class="text-xs text-destructive">{{ error }}</p>
      </CardContent>
    </Card>

    <div class="grid gap-4 sm:grid-cols-2">
      <Card
        v-for="n in visible"
        :key="n.id"
        class="cursor-pointer transition hover:border-primary/40"
        @click="router.push(`/novels/${n.id}`)"
      >
        <div class="flex gap-3 p-4 pb-0">
          <div
            class="flex h-24 w-16 shrink-0 items-center justify-center overflow-hidden rounded border bg-muted text-[10px] text-muted-foreground"
          >
            <img
              v-if="coverSrc(n)"
              :src="coverSrc(n)"
              class="h-full w-full object-cover"
              alt=""
            />
            <span v-else>{{ t("workspace.cover") }}</span>
          </div>
          <div class="min-w-0 flex-1 space-y-1">
            <CardTitle class="text-base leading-tight">{{ n.title }}</CardTitle>
            <p class="text-xs text-muted-foreground">
              {{ t("novels.wordsPerChapter", { min: n.word_count_min, max: n.word_count_max }) }} ·
              {{ t("novels.chapterCount", { n: n.chapter_count || 20 }) }} ·
              {{ t("novels.updated") }}
              {{ n.updated_at.slice(0, 19).replace("T", " ") }}
            </p>
            <p class="line-clamp-2 text-sm text-muted-foreground">
              {{ n.synopsis || t("novels.noSynopsis") }}
            </p>
          </div>
        </div>
        <CardContent class="pt-3">
          <div class="flex flex-wrap gap-2" @click.stop>
            <Button
              v-if="!n.archived"
              size="sm"
              variant="outline"
              @click="archive(n, true, $event)"
            >
              {{ t("novels.archive") }}
            </Button>
            <template v-else>
              <Button size="sm" variant="outline" @click="archive(n, false, $event)">
                {{ t("novels.unarchive") }}
              </Button>
              <Button size="sm" variant="destructive" @click="askDelete(n, $event)">
                {{ t("novels.delete") }}
              </Button>
            </template>
          </div>
        </CardContent>
      </Card>
    </div>
    <p v-if="!visible.length" class="text-sm text-muted-foreground">
      {{ tab === "archived" ? t("novels.emptyArchived") : t("novels.emptyActive") }}
    </p>

    <div
      v-if="pendingDelete"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelDelete"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("novels.delete") }}</h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{ t("novels.deleteConfirm", { title: pendingDelete.title }) }}
        </p>
        <p class="mt-2 text-sm font-medium text-destructive">
          {{ t("novels.deleteConfirmAgain", { title: pendingDelete.title }) }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="deleting" @click="cancelDelete">
            {{ t("novels.cancel") }}
          </Button>
          <Button variant="destructive" :disabled="deleting" @click="confirmDelete">
            {{ deleting ? t("novels.deleting") : t("novels.delete") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
