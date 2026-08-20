<script setup lang="ts">
import { reactive, watch } from "vue";
import type { CharacterCard } from "@/lib/api";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

const props = defineProps<{
  name: string;
  card: CharacterCard;
  busy?: boolean;
}>();

const emit = defineEmits<{
  save: [payload: { name: string; card: CharacterCard }];
}>();

const { t } = useI18n();

const draft = reactive({
  name: props.name,
  role: props.card.role || "",
  gender: props.card.gender || "",
  alignment: props.card.alignment || "",
  personality: props.card.personality || "",
  style: props.card.style || "",
  motto: props.card.motto || "",
});

watch(
  () => [props.name, props.card] as const,
  () => {
    draft.name = props.name;
    draft.role = props.card.role || "";
    draft.gender = props.card.gender || "";
    draft.alignment = props.card.alignment || "";
    draft.personality = props.card.personality || "";
    draft.style = props.card.style || "";
    draft.motto = props.card.motto || "";
  },
  { deep: true },
);

function onSave() {
  emit("save", {
    name: draft.name.trim() || props.name,
    card: {
      role: draft.role.trim(),
      gender: draft.gender.trim(),
      alignment: draft.alignment.trim(),
      personality: draft.personality.trim(),
      style: draft.style.trim(),
      motto: draft.motto.trim(),
    },
  });
}
</script>

<template>
  <div class="space-y-3 rounded-xl border bg-card p-4 shadow-sm">
    <div>
      <h3 class="text-base font-semibold">{{ t("character.editTitle") }}</h3>
      <p class="mt-0.5 text-xs text-muted-foreground">{{ t("character.subtitle") }}</p>
    </div>

    <div class="space-y-2 text-sm">
      <div>
        <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.name") }}</label>
        <Input v-model="draft.name" class="h-8" :disabled="busy" />
      </div>
      <div class="grid gap-2 sm:grid-cols-2">
        <div>
          <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.role") }}</label>
          <Input v-model="draft.role" class="h-8" :disabled="busy" />
        </div>
        <div>
          <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.gender") }}</label>
          <Input
            v-model="draft.gender"
            class="h-8"
            :placeholder="t('character.genderUnset')"
            :disabled="busy"
          />
        </div>
      </div>
      <div>
        <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.alignment") }}</label>
        <Input v-model="draft.alignment" class="h-8" :disabled="busy" />
      </div>
      <div>
        <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.personality") }}</label>
        <Textarea v-model="draft.personality" rows="3" class="text-sm" :disabled="busy" />
      </div>
      <div>
        <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.style") }}</label>
        <Textarea v-model="draft.style" rows="2" class="text-sm" :disabled="busy" />
      </div>
      <div>
        <label class="mb-1 block text-xs text-muted-foreground">{{ t("character.motto") }}</label>
        <Input v-model="draft.motto" class="h-8" :disabled="busy" />
      </div>
    </div>

    <div class="flex justify-end">
      <Button size="sm" :disabled="busy" @click="onSave">
        {{ busy ? t("character.saving") : t("character.save") }}
      </Button>
    </div>
  </div>
</template>
