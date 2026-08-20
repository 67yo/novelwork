<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, type KnowledgeBook, type KnowledgeChunk, type PublicKnowledgeCard } from "@/lib/api";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

type ModelProgress = {
  phase?: string;
  file?: string;
  fileIndex?: number;
  fileTotal?: number;
  bytesDownloaded?: number;
  bytesTotal?: number | null;
  done?: number;
  total?: number;
  percent?: number;
};

const { t } = useI18n();
const books = ref<KnowledgeBook[]>([]);
const publicCards = ref<PublicKnowledgeCard[]>([]);
const genres = ref<string[]>([]);
const selectedGenres = ref<string[]>([]);
const path = ref("");
const url = ref("");
const busy = ref(false);
const error = ref("");
const showImport = ref(false);
const section = ref<"sources" | "cards">("sources");
const statusTab = ref<"active" | "archived">("active");
const importMode = ref<"file" | "url">("file");
const fileDropHover = ref(false);
let unlistenFileDrop: (() => void) | null = null;
let unlistenModelProgress: UnlistenFn | null = null;

const modelProgress = ref<ModelProgress | null>(null);

const modelProgressLabel = computed(() => {
  const p = modelProgress.value;
  if (!p?.phase) return "";
  if (p.phase === "download") return t("library.modelDownloading");
  if (p.phase === "load" || p.phase === "ready") return t("library.modelLoading");
  if (p.phase === "embed") return t("library.modelEmbedding");
  if (p.phase === "done") return t("library.modelDone");
  return "";
});

const modelProgressPercent = computed(() =>
  Math.max(0, Math.min(100, Math.round(modelProgress.value?.percent ?? 0))),
);

const modelProgressDetail = computed(() => {
  const p = modelProgress.value;
  if (!p) return "";
  if (p.phase === "download" && p.file) {
    const file = p.file.split("/").pop() || p.file;
    if (p.bytesTotal && p.bytesTotal > 0) {
      return t("library.modelDownloadBytes", {
        file,
        done: formatMb(p.bytesDownloaded ?? 0),
        total: formatMb(p.bytesTotal),
      });
    }
    return t("library.modelDownloadFile", {
      file,
      index: p.fileIndex ?? 0,
      total: p.fileTotal ?? 0,
    });
  }
  if (p.phase === "embed" && p.total) {
    return t("library.modelEmbedDetail", { done: p.done ?? 0, total: p.total });
  }
  return "";
});

function formatMb(n: number) {
  return (n / (1024 * 1024)).toFixed(1);
}

function resetModelProgress() {
  modelProgress.value = null;
}

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
const indexStatus = ref<{ bookId: string; chunkCount: number; embeddingCount: number } | null>(
  null,
);
const indexBusy = ref(false);

const pendingDelete = ref<KnowledgeBook | null>(null);
const deleting = ref(false);
const reextractId = ref("");
const reextractTarget = ref<KnowledgeBook | null>(null);
const reextractError = ref("");

const visible = computed(() =>
  books.value.filter((b) => (statusTab.value === "archived" ? b.archived : !b.archived)),
);

const visibleCards = computed(() =>
  publicCards.value.filter((c) => (statusTab.value === "archived" ? c.archived : !c.archived)),
);

const bookTitleById = computed(() => {
  const m = new Map<string, string>();
  for (const b of books.value) m.set(b.id, b.title);
  return m;
});

function cardBookLabels(c: PublicKnowledgeCard): string {
  const names = c.book_ids
    .map((id) => bookTitleById.value.get(id) || id)
    .filter(Boolean);
  return names.join("、");
}

function toggleImport() {
  showImport.value = !showImport.value;
  error.value = "";
}

async function refresh() {
  books.value = await api.listKnowledge();
  genres.value = await api.listGenres();
  publicCards.value = await api.listPublicKnowledgeCards().catch(() => []);
}

function acceptImportPath(p: string) {
  const lower = p.toLowerCase();
  if (lower.endsWith(".pdf")) {
    error.value = t("library.noPdf");
    return;
  }
  if (!lower.endsWith(".txt") && !lower.endsWith(".epub")) {
    error.value = t("library.badFileType");
    return;
  }
  path.value = p;
  importMode.value = "file";
  error.value = "";
}

async function pick() {
  const p = await api.pickTextFile();
  if (p) acceptImportPath(p);
}

const fileName = computed(() => {
  const p = path.value;
  if (!p) return "";
  const parts = p.split(/[/\\]/);
  return parts[parts.length - 1] || p;
});

onMounted(async () => {
  await refresh();
  unlistenModelProgress = await listen<ModelProgress>("knowledge-model-progress", (ev) => {
    modelProgress.value = ev.payload ?? null;
  });
  unlistenFileDrop = await getCurrentWebview().onDragDropEvent((ev) => {
    if (!showImport.value || section.value !== "sources" || statusTab.value !== "active" || importMode.value !== "file") {
      fileDropHover.value = false;
      return;
    }
    const payload = ev.payload;
    if (payload.type === "enter" || payload.type === "over") {
      fileDropHover.value = true;
      return;
    }
    if (payload.type === "leave") {
      fileDropHover.value = false;
      return;
    }
    fileDropHover.value = false;
    const dropped = payload.paths[0];
    if (dropped) acceptImportPath(dropped);
  });
});

onUnmounted(() => {
  unlistenFileDrop?.();
  unlistenFileDrop = null;
  unlistenModelProgress?.();
  unlistenModelProgress = null;
});

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
  resetModelProgress();
  try {
    if (usingUrl) {
      await api.importKnowledgeUrl(u, selectedGenres.value);
      url.value = "";
    } else {
      await api.importKnowledge(path.value, selectedGenres.value);
      path.value = "";
    }
    showImport.value = false;
    statusTab.value = "active";
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
    resetModelProgress();
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
  indexStatus.value = null;
  chunksBusy.value = true;
  try {
    const [list, st] = await Promise.all([
      api.listKnowledgeChunks(b.id),
      api.knowledgeIndexStatus(b.id),
    ]);
    chunks.value = list;
    indexStatus.value = st;
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
  indexStatus.value = null;
}

async function rebuildIndex() {
  const b = viewing.value;
  if (!b || indexBusy.value) return;
  indexBusy.value = true;
  chunksError.value = "";
  resetModelProgress();
  try {
    indexStatus.value = await api.rebuildKnowledgeIndex(b.id);
  } catch (e) {
    chunksError.value = String(e);
  } finally {
    indexBusy.value = false;
    resetModelProgress();
  }
}

/** Chunk bodies often start with 【第N章 …】 — surface as source hint. */
function chunkChapterHint(content: string): string {
  const line = (content || "").split("\n")[0]?.trim() || "";
  if (line.startsWith("【第") && line.includes("章")) {
    return line.replace(/^【/, "").replace(/】$/, "").slice(0, 40);
  }
  return "";
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

async function archiveCard(c: PublicKnowledgeCard, archived: boolean) {
  error.value = "";
  try {
    await api.archivePublicKnowledgeCard(c.id, archived);
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

const editingCard = ref<PublicKnowledgeCard | null>(null);
const cardTitle = ref("");
const cardExtracted = ref("");
const pendingDeleteCard = ref<PublicKnowledgeCard | null>(null);

function startEditCard(c: PublicKnowledgeCard) {
  editingCard.value = c;
  cardTitle.value = c.title;
  cardExtracted.value = c.extracted;
  editError.value = "";
}

function startNewCard() {
  editingCard.value = {
    id: "",
    title: "",
    book_ids: [],
    extract_prompt: "",
    extracted: "",
    created_at: "",
    updated_at: "",
    archived: false,
  };
  cardTitle.value = "";
  cardExtracted.value = "";
  editError.value = "";
}

function cancelEditCard() {
  if (editBusy.value) return;
  editingCard.value = null;
  editError.value = "";
}

async function saveEditCard() {
  const c = editingCard.value;
  if (!c) return;
  const title = cardTitle.value.trim();
  if (!title) {
    editError.value = t("library.editTitleRequired");
    return;
  }
  editBusy.value = true;
  editError.value = "";
  try {
    await api.upsertPublicKnowledgeCard({
      id: c.id || null,
      title,
      book_ids: c.book_ids,
      extract_prompt: c.extract_prompt,
      extracted: cardExtracted.value,
    });
    editingCard.value = null;
    await refresh();
  } catch (e) {
    editError.value = String(e);
  } finally {
    editBusy.value = false;
  }
}

function askDeleteCard(c: PublicKnowledgeCard) {
  pendingDeleteCard.value = c;
  error.value = "";
}

function cancelDeleteCard() {
  if (deleting.value) return;
  pendingDeleteCard.value = null;
}

async function confirmDeleteCard() {
  const c = pendingDeleteCard.value;
  if (!c) return;
  deleting.value = true;
  try {
    await api.deletePublicKnowledgeCard(c.id);
    pendingDeleteCard.value = null;
    if (editingCard.value?.id === c.id) editingCard.value = null;
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

function openReextract(b: KnowledgeBook) {
  if (reextractId.value || busy.value) return;
  reextractTarget.value = b;
  reextractError.value = "";
  error.value = "";
}

function cancelReextract() {
  if (reextractId.value) return;
  reextractTarget.value = null;
  reextractError.value = "";
}

async function confirmReextract() {
  const b = reextractTarget.value;
  if (!b || reextractId.value || busy.value) return;
  reextractId.value = b.id;
  reextractError.value = "";
  error.value = "";
  resetModelProgress();
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
    reextractTarget.value = null;
    await refresh();
    if (viewing.value?.id === updated.id) {
      viewing.value = updated;
      await openChunks(updated);
    }
  } catch (e) {
    reextractError.value = String(e);
  } finally {
    reextractId.value = "";
    resetModelProgress();
  }
}
</script>

<template>
  <div class="mx-auto h-full max-w-5xl space-y-6 overflow-y-auto overscroll-contain p-6">
    <div class="flex items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("library.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ section === "cards" ? t("library.subtitleCards") : t("library.subtitle") }}
        </p>
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <div class="flex gap-1 text-sm">
        <button
          type="button"
          class="rounded-md px-3 py-1.5"
          :class="section === 'sources' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
          @click="section = 'sources'"
        >
          {{ t("library.tabSources") }}
        </button>
        <button
          type="button"
          class="rounded-md px-3 py-1.5"
          :class="section === 'cards' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
          @click="section = 'cards'"
        >
          {{ t("library.tabCards") }}
        </button>
      </div>
      <div
        class="flex rounded-md border p-0.5 text-xs"
        role="tablist"
        :aria-label="t('library.statusFilter')"
      >
        <button
          type="button"
          class="rounded px-2.5 py-1"
          :class="statusTab === 'active' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
          @click="statusTab = 'active'"
        >
          {{ t("library.tabActive") }}
        </button>
        <button
          type="button"
          class="rounded px-2.5 py-1"
          :class="statusTab === 'archived' ? 'bg-accent' : 'text-muted-foreground hover:bg-muted'"
          @click="statusTab = 'archived'"
        >
          {{ t("library.tabArchived") }}
        </button>
      </div>
      <div class="flex-1" />
      <Button v-if="section === 'sources' && statusTab === 'active'" size="sm" @click="toggleImport">
        {{ showImport ? t("library.cancel") : t("library.import") }}
      </Button>
      <Button v-if="section === 'cards' && statusTab === 'active'" size="sm" @click="startNewCard">
        {{ t("library.newCard") }}
      </Button>
    </div>

    <template v-if="section === 'sources'">
    <Card v-if="showImport && statusTab === 'active'">
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
        <div v-if="importMode === 'file'" class="space-y-2">
          <button
            type="button"
            class="flex w-full flex-col items-center justify-center gap-1 rounded-lg border border-dashed px-4 py-8 text-center transition-colors"
            :class="
              fileDropHover
                ? 'border-primary bg-accent text-foreground'
                : 'border-border bg-muted/30 text-muted-foreground hover:border-primary/50 hover:bg-muted/50'
            "
            @click="pick"
          >
            <span class="text-sm">{{ t("library.dropOrPick") }}</span>
            <span v-if="path" class="max-w-full truncate text-xs text-foreground" :title="path">
              {{ t("library.fileSelected", { name: fileName }) }}
            </span>
          </button>
          <div class="flex gap-2">
            <Input v-model="path" :placeholder="t('library.path')" readonly class="flex-1" />
            <Button variant="outline" @click="pick">{{ t("library.pick") }}</Button>
          </div>
        </div>
        <div v-else>
          <label class="mb-1 block text-sm">{{ t("library.url") }}</label>
          <Input v-model="url" :placeholder="t('library.urlPh')" class="w-full" />
          <p class="mt-1 text-xs text-muted-foreground">{{ t("library.urlHint") }}</p>
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
        <div v-if="busy && modelProgress" class="space-y-2 rounded-md border bg-muted/30 p-3">
          <div class="flex items-center justify-between gap-2 text-sm">
            <span>{{ modelProgressLabel }}</span>
            <span class="tabular-nums text-muted-foreground">{{ modelProgressPercent }}%</span>
          </div>
          <div class="h-2 overflow-hidden rounded-full bg-muted">
            <div
              class="h-full rounded-full bg-primary transition-[width] duration-200"
              :style="{ width: `${modelProgressPercent}%` }"
            />
          </div>
          <p v-if="modelProgressDetail" class="truncate text-xs text-muted-foreground">
            {{ modelProgressDetail }}
          </p>
        </div>
        <Button :disabled="busy" @click="doImport">
          {{ busy ? t("library.importing") : t("library.doImport") }}
        </Button>
      </CardContent>
    </Card>

    <p v-if="error && !showImport" class="text-sm text-destructive">{{ error }}</p>

    <div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-4">
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
              @click="openReextract(b)"
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
      {{ statusTab === "archived" ? t("library.emptyArchived") : t("library.empty") }}
    </p>
    </template>

    <template v-else>
      <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-4">
        <Card v-for="c in visibleCards" :key="c.id">
          <CardHeader>
            <CardTitle class="text-base">{{ c.title }}</CardTitle>
            <p v-if="c.book_ids.length" class="text-sm text-muted-foreground">
              {{ t("library.cardBooks") }} · {{ cardBookLabels(c) }}
            </p>
          </CardHeader>
          <CardContent class="space-y-3 text-sm">
            <p class="line-clamp-4 text-muted-foreground">
              {{ (c.extracted || "").trim() || t("library.cardEmptyExtracted") }}
            </p>
            <div class="flex flex-wrap gap-2">
              <Button size="sm" variant="outline" @click="startEditCard(c)">
                {{ t("library.edit") }}
              </Button>
              <Button
                v-if="!c.archived"
                size="sm"
                variant="outline"
                @click="archiveCard(c, true)"
              >
                {{ t("library.archive") }}
              </Button>
              <template v-else>
                <Button size="sm" variant="outline" @click="archiveCard(c, false)">
                  {{ t("library.unarchive") }}
                </Button>
                <Button size="sm" variant="destructive" @click="askDeleteCard(c)">
                  {{ t("library.delete") }}
                </Button>
              </template>
            </div>
          </CardContent>
        </Card>
      </div>
      <p v-if="!visibleCards.length" class="text-sm text-muted-foreground">
        {{ statusTab === "archived" ? t("library.emptyArchivedCards") : t("library.emptyCards") }}
      </p>
    </template>

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
              <p v-if="indexStatus" class="mt-1 text-xs text-muted-foreground">
                {{
                  t("library.indexStatus", {
                    emb: indexStatus.embeddingCount,
                    chunks: indexStatus.chunkCount,
                  })
                }}
              </p>
              <div v-if="indexBusy && modelProgress" class="mt-2 space-y-1.5">
                <div class="flex items-center justify-between gap-2 text-xs">
                  <span>{{ modelProgressLabel }}</span>
                  <span class="tabular-nums text-muted-foreground">{{ modelProgressPercent }}%</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    class="h-full rounded-full bg-primary transition-[width] duration-200"
                    :style="{ width: `${modelProgressPercent}%` }"
                  />
                </div>
                <p v-if="modelProgressDetail" class="truncate text-[11px] text-muted-foreground">
                  {{ modelProgressDetail }}
                </p>
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-1">
              <Button
                size="sm"
                variant="outline"
                :disabled="indexBusy || chunksBusy"
                @click="rebuildIndex"
              >
                {{ indexBusy ? t("library.rebuildingIndex") : t("library.rebuildIndex") }}
              </Button>
              <Button size="sm" variant="ghost" @click="closeChunks">{{ t("library.close") }}</Button>
            </div>
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
              <span v-if="chunkChapterHint(c.content)" class="normal-case">
                · {{ chunkChapterHint(c.content) }}
              </span>
            </div>
            <pre class="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{{ c.content }}</pre>
          </div>
        </CardContent>
      </Card>
    </div>

    <div
      v-if="reextractTarget"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelReextract"
    >
      <div
        class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <h2 class="text-base font-semibold">{{ t("library.reextract") }}</h2>
        <p class="mt-3 text-sm text-muted-foreground">
          {{ t("library.reextractPanelHint", { title: reextractTarget.title }) }}
        </p>
        <div v-if="reextractId && modelProgress" class="mt-3 space-y-2">
          <div class="flex items-center justify-between gap-2 text-sm">
            <span>{{ modelProgressLabel }}</span>
            <span class="tabular-nums text-muted-foreground">{{ modelProgressPercent }}%</span>
          </div>
          <div class="h-2 overflow-hidden rounded-full bg-muted">
            <div
              class="h-full rounded-full bg-primary transition-[width] duration-200"
              :style="{ width: `${modelProgressPercent}%` }"
            />
          </div>
          <p v-if="modelProgressDetail" class="truncate text-xs text-muted-foreground">
            {{ modelProgressDetail }}
          </p>
        </div>
        <p v-if="reextractError" class="mt-2 text-sm text-destructive">{{ reextractError }}</p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="!!reextractId" @click="cancelReextract">
            {{ t("library.cancel") }}
          </Button>
          <Button :disabled="!!reextractId" @click="confirmReextract">
            {{ reextractId ? t("library.reextracting") : t("library.reextractConfirm") }}
          </Button>
        </div>
      </div>
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

    <div
      v-if="editingCard"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelEditCard"
    >
      <div
        class="flex max-h-[90vh] w-full max-w-lg flex-col overflow-hidden rounded-lg border bg-background shadow-lg"
        role="dialog"
        aria-modal="true"
      >
        <div class="shrink-0 border-b px-5 py-4">
          <h2 class="text-base font-semibold">
            {{ editingCard.id ? t("library.edit") : t("library.newCardTitle") }}
          </h2>
        </div>
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <div>
            <label class="mb-1 block text-sm">{{ t("library.editTitle") }}</label>
            <Input v-model="cardTitle" class="h-9" />
          </div>
          <p v-if="editingCard.book_ids.length" class="text-xs text-muted-foreground">
            {{ t("library.cardBooks") }} · {{ cardBookLabels(editingCard) }}
          </p>
          <div>
            <label class="mb-1 block text-sm">{{ t("library.cardExtracted") }}</label>
            <Textarea
              v-model="cardExtracted"
              rows="12"
              class="min-h-[12rem] text-sm"
              :placeholder="t('library.cardExtractedPh')"
            />
          </div>
          <p v-if="editError" class="text-sm text-destructive">{{ editError }}</p>
        </div>
        <div class="flex shrink-0 justify-end gap-2 border-t px-5 py-3">
          <Button variant="outline" :disabled="editBusy" @click="cancelEditCard">
            {{ t("library.cancel") }}
          </Button>
          <Button :disabled="editBusy" @click="saveEditCard">
            {{ editBusy ? t("library.saving") : t("library.renameSave") }}
          </Button>
        </div>
      </div>
    </div>

    <div
      v-if="pendingDeleteCard"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="cancelDeleteCard"
    >
      <div class="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg" role="dialog" aria-modal="true">
        <h2 class="text-base font-semibold">{{ t("library.delete") }}</h2>
        <p class="mt-3 whitespace-pre-wrap text-sm text-muted-foreground">
          {{ t("library.deleteCardConfirm", { title: pendingDeleteCard.title }) }}
        </p>
        <p class="mt-2 text-sm font-medium text-destructive">
          {{ t("library.deleteCardConfirmAgain", { title: pendingDeleteCard.title }) }}
        </p>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" :disabled="deleting" @click="cancelDeleteCard">
            {{ t("library.cancel") }}
          </Button>
          <Button variant="destructive" :disabled="deleting" @click="confirmDeleteCard">
            {{ deleting ? t("library.deleting") : t("library.delete") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>
