<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api, type KnowledgeBook } from "@/lib/api";
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
  if (path.value.toLowerCase().endsWith(".epub") || path.value.toLowerCase().endsWith(".pdf")) {
    error.value = t("library.onlyTxt");
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    await api.importKnowledge(path.value, prompt.value, selectedGenres.value);
    showImport.value = false;
    path.value = "";
    await refresh();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
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

    <Card v-if="showImport">
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

    <div v-if="!books.length" class="rounded-xl border border-dashed p-10 text-center text-sm text-muted-foreground">
      {{ t("library.empty") }}
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <Card v-for="b in books" :key="b.id">
        <CardHeader>
          <CardTitle class="text-base">{{ b.title }}</CardTitle>
          <p class="text-sm text-muted-foreground">
            {{ b.author }} · {{ t("library.chunks", { n: b.chunk_count }) }}
          </p>
        </CardHeader>
        <CardContent class="space-y-2 text-sm">
          <div class="flex flex-wrap gap-1">
            <span v-for="g in b.genres" :key="g" class="rounded bg-muted px-1.5 py-0.5 text-xs">{{ g }}</span>
          </div>
          <p class="line-clamp-2 text-muted-foreground">{{ b.extract_prompt || "—" }}</p>
        </CardContent>
      </Card>
    </div>
  </div>
</template>
