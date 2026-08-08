<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
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
const url = ref("");
const prompt = ref("");
const busy = ref(false);
const error = ref("");
const showImport = ref(false);
const tab = ref<"active" | "archived">("active");
const importMode = ref<"file" | "url">("file");

const editing = ref<KnowledgeBook | null>(null);
const editTitle = ref("");
const editAuthor = ref("");
const editBlurb = ref("");
const editGenres = ref<string[]>([]);
const customGenre = ref("");
const editBusy = ref(false);
const editError = ref("");

const editGenreOptions = computed(() => {
  const out = [...genres.value];
  for (const g of editGenres.value) {
    if (!out.includes(g)) out.push(g);
  }
  return out;
});

const viewing = ref<KnowledgeBook | null>(null);
const chunks = ref<KnowledgeChunk[]>([]);
const chunksBusy = ref(false);
const chunksError = ref("");

const pendingDelete = ref<KnowledgeBook | null>(null);
const deleting = ref(false);
const reextractId = ref("");

const visible = computed(() =>
  books.value.filter((b) => (tab.value === "archived" ? b.archived : !b.archived)),
);

async function refresh() {
  books.value = await api.listKnowledge();
  genres.value = await api.listGenres();
  syncDefaultPrompt();
}

function syncDefaultPrompt() {
  const bookDefault = t("library.defaultPrompt");
  const urlDefault = t("library.defaultUrlPrompt");
  if (!prompt.value || prompt.value === bookDefault || prompt.value === urlDefault) {
    prompt.value = importMode.value === "url" ? urlDefault : bookDefault;
  }
}

watch(importMode, syncDefaultPrompt);

onMounted(refresh);

async function pick() {
  const p = await api.pickTextFile();
  if (p) {
    path.value = p;
    importMode.value = "file";
  }
}

function toggleGenre(g: string) {
  if (selectedGenres.value.includes(g)) {
    selectedGenres.value = selectedGenres.value.filter((x) => x !== g);
  } else {
    selectedGenres.value = [...selectedGenres.value, g];
  }
}

async function doImport() {
  const u = url.value.trim();
  const usingUrl = importMode.value === "url" || (!!u && !path.value);
  if (usingUrl) {
    if (!u) {
      error.value = t("library.needUrl");
      return;
    }
    if (!(u.startsWith("http://") || u.startsWith("https://"))) {
      error.value = t("library.badUrl");
      return;
    }
  } else {
    if (!path.value) {
      error.value = t("library.needFile");
      return;
    }
    if (path.value.toLowerCase().endsWith(".pdf")) {
      error.value = t("library.noPdf");
      return;
    }
  }
  busy.value = true;
  error.value = "";
  try {
    if (usingUrl) {
      await api.importKnowledgeUrl(u, prompt.value, selectedGenres.value);
      url.value = "";
    } else {
      await api.importKnowledge(path.value, prompt.value, selectedGenres.value);
      path.value = "";
    }
    showImport.value = false;
    tab.value = "active";
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function startEdit(b: KnowledgeBook) {
  editing.value = b;
  editTitle.value = b.title;
  editAuthor.value = b.author;
  editBlurb.value = b.extract_prompt;
  editGenres.value = [...b.genres];
  customGenre.value = "";
  editError.value = "";
}

function cancelEdit() {
  if (editBusy.value) return;
  editing.value = null;
  editError.value = "";
}

function toggleEditGenre(g: string) {
  if (editGenres.value.includes(g)) {
    editGenres.value = editGenres.value.filter((x) => x !== g);
  } else {
    editGenres.value = [...editGenres.value, g];
  }
}

function addCustomGenre() {
  const g = customGenre.value.trim();
  if (!g) return;
  if (!editGenres.value.includes(g)) editGenres.value = [...editGenres.value, g];
  if (!genres.value.includes(g)) genres.value = [...genres.value, g];
  customGenre.value = "";
}

async function saveEdit() {
  const b = editing.value;
  if (!b) return;
  const title = editTitle.value.trim();
  if (!title) {
    editError.value = t("library.editTitleRequired");
    return;
  }
  editBusy.value = true;
  editError.value = "";
  try {
    const updated = await api.updateKnowledge(
      b.id,
      title,
      editAuthor.value.trim(),
      editBlurb.value,
      editGenres.value,
    );
    editing.value = null;
    await refresh();
    if (viewing.value?.id === updated.id) {
      viewing.value = { ...viewing.value, ...updated };
    }
  } catch (e) {
    editError.value = String(e);
  } finally {
    editBusy.value = false;
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

function isHttpSource(p: string) {
  const s = p.trim().toLowerCase();
  return s.startsWith("http://") || s.startsWith("https://");
}

async function reextract(b: KnowledgeBook) {
  if (reextractId.value || busy.value) return;
  reextractId.value = b.id;
  error.value = "";
  try {
    let updated: KnowledgeBook;
    try {
      updated = await api.reextractKnowledge(b.id);
    } catch (e) {
      const msg = String(e);
      if (isHttpSource(b.source_path) || !msg.includes("源文件不存在")) throw e;
      const p = await api.pickTextFile();
      if (!p) return;
      updated = await api.reextractKnowledge(b.id, p);
    }
    await refresh();
    if (viewing.value?.id === updated.id) {
      viewing.value = updated;
      await openChunks(updated);
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    reextractId.value = "";
  }
}
</script>

<template>
  <div class="mx-auto h-full max-w-5xl space-y-6 overflow-y-auto overscroll-contain p-6">
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
        <div class="flex gap-2 text-sm">
          <button
            type="button"
            class="rounded-md px-3 py-1.5"
            :class="importMode === 'file' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
            @click="importMode = 'file'"
          >
            {{ t("library.importModeFile") }}
          </button>
          <button
            type="button"
            class="rounded-md px-3 py-1.5"
            :class="importMode === 'url' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
            @click="importMode = 'url'"
          >
            {{ t("library.importModeUrl") }}
          </button>
        </div>
        <div v-if="importMode === 'file'" class="flex gap-2">
          <Input v-model="path" :placeholder="t('library.path')" readonly class="flex-1" />
          <Button variant="outline" @click="pick">{{ t("library.pick") }}</Button>
        </div>
        <div v-else>
          <label class="mb-1 block text-sm">{{ t("library.url") }}</label>
          <Input v-model="url" :placeholder="t('library.urlPh')" class="w-full" />
          <p class="mt-1 text-xs text-muted-foreground">{{ t("library.urlHint") }}</p>
        </div>
        <div>
          <label class="mb-1 block text-sm">{{ t("library.extractPrompt") }}</label>
          <Textarea v-model="prompt" rows="3" />
        </div>
        <div>
          <label class="mb-2 block text-sm">{{ t("library.genres") }}</label>
          <div class="flex max-h-28 flex-wrap gap-2 overflow-y-auto overscroll-contain">
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
          <CardTitle class="text-base">{{ b.title }}</CardTitle>
          <p class="text-sm text-muted-foreground">
            {{ b.author }} · {{ t("library.chunks", { n: b.chunk_count }) }}
          </p>
        </CardHeader>
        <CardContent class="space-y-3 text-sm">
          <div class="flex flex-wrap items-center gap-1">
            <span v-for="g in b.genres" :key="g" class="rounded bg-muted px-1.5 py-0.5 text-xs">{{ g }}</span>
            <span v-if="!b.genres.length" class="text-xs text-muted-foreground">{{ t("library.noGenres") }}</span>
          </div>
          <p class="line-clamp-2 text-muted-foreground">{{ b.extract_prompt || "—" }}</p>
          <div class="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" @click="startEdit(b)">
              {{ t("library.edit") }}
            </Button>
            <Button
              size="sm"
              variant="outline"
              :disabled="!!reextractId || busy"
              :title="t('library.reextractHint')"
              @click="reextract(b)"
            >
              {{ reextractId === b.id ? t("library.reextracting") : t("library.reextract") }}
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
      v-if="editing"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelEdit"
    >
      <div
        class="flex max-h-[90vh] w-full max-w-lg flex-col overflow-hidden rounded-lg border bg-background shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <div class="shrink-0 border-b px-5 py-4">
          <h2 class="text-base font-semibold">{{ t("library.edit") }}</h2>
        </div>
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <div>
            <label class="mb-1 block text-sm">{{ t("library.editTitle") }}</label>
            <Input v-model="editTitle" class="h-9" />
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("library.editAuthor") }}</label>
            <Input v-model="editAuthor" class="h-9" />
          </div>
          <div>
            <label class="mb-1 block text-sm">{{ t("library.editBlurb") }}</label>
            <Textarea v-model="editBlurb" rows="4" :placeholder="t('library.editBlurbPh')" />
          </div>
          <div>
            <label class="mb-2 block text-sm">{{ t("library.editGenres") }}</label>
            <div class="mb-2 flex max-h-32 flex-wrap gap-1.5 overflow-y-auto rounded-md border p-2">
              <button
                v-for="g in editGenreOptions"
                :key="g"
                type="button"
                class="rounded-md border px-2 py-0.5 text-xs"
                :class="editGenres.includes(g) ? 'border-primary bg-accent' : 'border-border'"
                @click="toggleEditGenre(g)"
              >
                {{ g }}
              </button>
            </div>
            <div class="flex gap-2">
              <Input
                v-model="customGenre"
                class="h-8 flex-1 text-xs"
                :placeholder="t('library.addGenrePh')"
                @keydown.enter.prevent="addCustomGenre"
              />
              <Button size="sm" variant="outline" class="h-8" @click="addCustomGenre">
                {{ t("library.addGenre") }}
              </Button>
            </div>
          </div>
          <p v-if="editError" class="text-sm text-destructive">{{ editError }}</p>
        </div>
        <div class="flex shrink-0 justify-end gap-2 border-t px-5 py-3">
          <Button variant="outline" :disabled="editBusy" @click="cancelEdit">
            {{ t("library.cancel") }}
          </Button>
          <Button :disabled="editBusy" @click="saveEdit">
            {{ editBusy ? t("library.saving") : t("library.renameSave") }}
          </Button>
        </div>
      </div>
    </div>

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
