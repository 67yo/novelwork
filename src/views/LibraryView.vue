<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { api, type KnowledgeBook, type KnowledgeChunk } from "@/lib/api";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

const { t } = useI18n();
const books = ref<KnowledgeBook[]>([]);
const genres = ref<string[]>([]);
const selectedGenres = ref<string[]>([]);
const path = ref("");
const prompt = ref("");
const busy = ref(false);
const error = ref("");
const showImport = ref(false);
const tab = ref<"active" | "archived">("active");

const renamingId = ref<string | null>(null);
const renameDraft = ref("");
const renameBusy = ref(false);

const viewing = ref<KnowledgeBook | null>(null);
const chunks = ref<KnowledgeChunk[]>([]);
const chunksBusy = ref(false);
const chunksError = ref("");

const pendingDelete = ref<KnowledgeBook | null>(null);
const deleting = ref(false);

const visible = computed(() =>
  books.value.filter((b) => (tab.value === "archived" ? b.archived : !b.archived)),
);

async function refresh() {
  books.value = await api.listKnowledge();
  genres.value = await api.listGenres();
  if (!prompt.value) prompt.value = t("library.defaultPrompt");
}

onMounted(refresh);

async function pick() {
  const p = await api.pickTextFile();
  if (p) path.value = p;
}

function toggleGenre(g: string) {
  if (selectedGenres.value.includes(g)) {
    selectedGenres.value = selectedGenres.value.filter((x) => x !== g);
  } else {
    selectedGenres.value = [...selectedGenres.value, g];
  }
}

async function doImport() {
  if (!path.value) {
    error.value = t("library.needFile");
    return;
  }
  if (path.value.toLowerCase().endsWith(".pdf")) {
    error.value = t("library.noPdf");
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    await api.importKnowledge(path.value, prompt.value, selectedGenres.value);
    showImport.value = false;
    path.value = "";
    tab.value = "active";
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function startRename(b: KnowledgeBook) {
  renamingId.value = b.id;
  renameDraft.value = b.title;
  void nextTick(() => {
    const el = document.getElementById(`kb-rename-${b.id}`) as HTMLInputElement | null;
    el?.focus();
    el?.select();
  });
}

function cancelRename() {
  renamingId.value = null;
  renameDraft.value = "";
}

async function saveRename(b: KnowledgeBook) {
  const title = renameDraft.value.trim();
  if (!title || title === b.title) {
    cancelRename();
    return;
  }
  renameBusy.value = true;
  error.value = "";
  try {
    await api.renameKnowledge(b.id, title);
    renamingId.value = null;
    await refresh();
    if (viewing.value?.id === b.id) {
      viewing.value = { ...viewing.value, title };
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    renameBusy.value = false;
  }
}

async function openChunks(b: KnowledgeBook) {
  viewing.value = b;
  chunks.value = [];
  chunksError.value = "";
  chunksBusy.value = true;
  try {
    chunks.value = await api.listKnowledgeChunks(b.id);
  } catch (e) {
    chunksError.value = String(e);
  } finally {
    chunksBusy.value = false;
  }
}

function closeChunks() {
  viewing.value = null;
  chunks.value = [];
  chunksError.value = "";
}

async function archive(b: KnowledgeBook, archived: boolean) {
  error.value = "";
  try {
    await api.archiveKnowledge(b.id, archived);
    if (viewing.value?.id === b.id) closeChunks();
    await refresh();
  } catch (e) {
    error.value = String(e);
  }
}

function askDelete(b: KnowledgeBook) {
  pendingDelete.value = b;
  error.value = "";
}

function cancelDelete() {
  if (deleting.value) return;
  pendingDelete.value = null;
}

async function confirmDelete() {
  const b = pendingDelete.value;
  if (!b) return;
  deleting.value = true;
  try {
    await api.deleteKnowledge(b.id);
    pendingDelete.value = null;
    if (viewing.value?.id === b.id) closeChunks();
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    deleting.value = false;
  }
}
</script>

<template>
  <div class="mx-auto max-w-5xl space-y-6 p-6">
    <div class="flex items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("library.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t("library.subtitle") }}</p>
      </div>
      <Button @click="showImport = !showImport">
        {{ showImport ? t("library.cancel") : t("library.import") }}
      </Button>
    </div>

    <div class="flex gap-2 text-sm">
      <button
        class="rounded-md px-3 py-1.5"
        :class="tab === 'active' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
        @click="tab = 'active'"
      >
        {{ t("library.tabActive") }}
      </button>
      <button
        class="rounded-md px-3 py-1.5"
        :class="tab === 'archived' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
        @click="tab = 'archived'"
      >
        {{ t("library.tabArchived") }}
      </button>
    </div>

    <Card v-if="showImport && tab === 'active'">
      <CardHeader>
        <CardTitle>{{ t("library.importTxt") }}</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="flex gap-2">
          <Input v-model="path" :placeholder="t('library.path')" readonly class="flex-1" />
          <Button variant="outline" @click="pick">{{ t("library.pick") }}</Button>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("library.extractPrompt") }}</label>
          <Textarea v-model="prompt" rows="3" />
        </div>
        <div>
          <label class="mb-2 block text-sm">{{ t("library.genres") }}</label>
          <div class="flex max-h-28 flex-wrap gap-2 overflow-y-auto">
            <button
              v-for="g in genres"
              :key="g"
              type="button"
              class="rounded-md border px-2 py-1 text-xs"
              :class="selectedGenres.includes(g) ? 'border-primary bg-accent' : 'border-border'"
              @click="toggleGenre(g)"
            >
              {{ g }}
            </button>
          </div>
        </div>
        <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
        <Button :disabled="busy" @click="doImport">
          {{ busy ? t("library.importing") : t("library.doImport") }}
        </Button>
      </CardContent>
    </Card>

    <p v-if="error && !showImport" class="text-sm text-destructive">{{ error }}</p>

    <div class="grid gap-4 sm:grid-cols-2">
      <Card v-for="b in visible" :key="b.id">
        <CardHeader>
          <div v-if="renamingId === b.id" class="flex gap-2">
            <Input
              :id="`kb-rename-${b.id}`"
              v-model="renameDraft"
              class="flex-1"
              @keydown.enter.prevent="saveRename(b)"
              @keydown.escape.prevent="cancelRename"
            />
            <Button size="sm" :disabled="renameBusy" @click="saveRename(b)">{{ t("library.renameSave") }}</Button>
            <Button size="sm" variant="ghost" :disabled="renameBusy" @click="cancelRename">{{ t("library.cancel") }}</Button>
          </div>
          <template v-else>
            <CardTitle class="text-base">{{ b.title }}</CardTitle>
            <p class="text-sm text-muted-foreground">
              {{ b.author }} · {{ t("library.chunks", { n: b.chunk_count }) }}
            </p>
          </template>
        </CardHeader>
        <CardContent class="space-y-3 text-sm">
          <div class="flex flex-wrap gap-1">
            <span v-for="g in b.genres" :key="g" class="rounded bg-muted px-1.5 py-0.5 text-xs">{{ g }}</span>
          </div>
          <p class="line-clamp-2 text-muted-foreground">{{ b.extract_prompt || "—" }}</p>
          <div class="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" :disabled="renamingId === b.id" @click="startRename(b)">
              {{ t("library.rename") }}
            </Button>
            <Button size="sm" variant="secondary" @click="openChunks(b)">
              {{ t("library.viewData") }}
            </Button>
            <Button
              v-if="!b.archived"
              size="sm"
              variant="outline"
              @click="archive(b, true)"
            >
              {{ t("library.archive") }}
            </Button>
            <template v-else>
              <Button size="sm" variant="outline" @click="archive(b, false)">
                {{ t("library.unarchive") }}
              </Button>
              <Button size="sm" variant="destructive" @click="askDelete(b)">
                {{ t("library.delete") }}
              </Button>
            </template>
          </div>
        </CardContent>
      </Card>
    </div>

    <p v-if="!visible.length" class="text-sm text-muted-foreground">
      {{ tab === "archived" ? t("library.emptyArchived") : t("library.empty") }}
    </p>

    <div
      v-if="viewing"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="closeChunks"
    >
      <Card class="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden">
        <CardHeader class="shrink-0 border-b">
          <div class="flex items-start justify-between gap-3">
            <div>
              <CardTitle class="text-base">{{ viewing.title }}</CardTitle>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ t("library.chunksView", { n: chunks.length || viewing.chunk_count }) }}
              </p>
            </div>
            <Button size="sm" variant="ghost" @click="closeChunks">{{ t("library.close") }}</Button>
          </div>
        </CardHeader>
        <CardContent class="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          <p v-if="chunksBusy" class="text-sm text-muted-foreground">{{ t("library.loadingChunks") }}</p>
          <p v-else-if="chunksError" class="text-sm text-destructive">{{ chunksError }}</p>
          <p v-else-if="!chunks.length" class="text-sm text-muted-foreground">{{ t("library.noChunks") }}</p>
          <div
            v-for="c in chunks"
            :key="c.idx"
            class="rounded-md border bg-muted/30 p-3"
          >
            <div class="mb-1 text-[10px] uppercase text-muted-foreground">
              {{ t("library.chunkIndex", { n: c.idx + 1 }) }}
            </div>
            <pre class="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{{ c.content }}</pre>
          </div>
        </CardContent>
      </Card>
    </div>

    <div
      v-if="pendingDelete"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelDelete"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("library.delete") }}</h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{ t("library.deleteConfirm", { title: pendingDelete.title }) }}
        </p>
        <p class="mt-2 text-sm font-medium text-destructive">
          {{ t("library.deleteConfirmAgain", { title: pendingDelete.title }) }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="deleting" @click="cancelDelete">
            {{ t("library.cancel") }}
          </Button>
          <Button variant="destructive" :disabled="deleting" @click="confirmDelete">
            {{ deleting ? t("library.deleting") : t("library.delete") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
