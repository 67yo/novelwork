<script setup lang="ts">
import type { CharacterCard } from "@/lib/api";
import { useI18n } from "@/i18n";

defineProps<{
  name: string;
  card: CharacterCard;
}>();

const { t } = useI18n();
</script>

<template>
  <div class="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
    <div class="flex items-start justify-between gap-3">
      <div>
        <h3 class="text-base font-semibold">{{ name }}</h3>
        <p class="mt-0.5 text-xs text-muted-foreground">{{ t("character.subtitle") }}</p>
      </div>
      <div class="flex flex-col items-end gap-1 text-xs">
        <span class="rounded-md bg-muted px-2 py-0.5">{{ card.gender || t("character.genderUnset") }}</span>
        <span class="rounded-md bg-amber-100 px-2 py-0.5 text-amber-900">{{ card.role }}</span>
      </div>
    </div>

    <dl class="grid gap-2 text-sm">
      <div class="grid grid-cols-[4.5rem_1fr] gap-2">
        <dt class="text-muted-foreground">{{ t("character.alignment") }}</dt>
        <dd>{{ card.alignment || "—" }}</dd>
      </div>
      <div class="grid grid-cols-[4.5rem_1fr] gap-2">
        <dt class="text-muted-foreground">{{ t("character.personality") }}</dt>
        <dd class="whitespace-pre-wrap">{{ card.personality || "—" }}</dd>
      </div>
      <div class="grid grid-cols-[4.5rem_1fr] gap-2">
        <dt class="text-muted-foreground">{{ t("character.style") }}</dt>
        <dd class="whitespace-pre-wrap">{{ card.style || "—" }}</dd>
      </div>
      <div class="grid grid-cols-[4.5rem_1fr] gap-2">
        <dt class="text-muted-foreground">{{ t("character.motto") }}</dt>
        <dd class="italic text-muted-foreground">「{{ card.motto || "—" }}」</dd>
      </div>
    </dl>

    <p class="text-[11px] text-muted-foreground">
      {{ t("character.hint") }}
    </p>
  </div>
</template>
