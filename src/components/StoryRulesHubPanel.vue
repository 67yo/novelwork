<script setup lang="ts">
import { computed } from "vue";
import { MessageSquare } from "@lucide/vue";
import { STORY_RULES_FAN_SLOTS } from "@/lib/storyRules";
import { useI18n } from "@/i18n";
import { Button } from "@/components/ui/button";

const props = defineProps<{
  blockLinks: { id: string; slot: string; title: string }[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  openBlock: [id: string];
  openChat: [];
}>();

const { t } = useI18n();

const orderedLinks = computed(() => {
  const bySlot = new Map(props.blockLinks.map((l) => [l.slot, l]));
  return STORY_RULES_FAN_SLOTS.map((def) => bySlot.get(def.slot)).filter(Boolean) as {
    id: string;
    slot: string;
    title: string;
  }[];
});
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto text-sm">
    <div class="flex items-center justify-between gap-2">
      <h3 class="text-sm font-semibold">{{ t("workspace.wv.storyRules") }}</h3>
      <Button type="button" size="sm" variant="outline" class="h-7 px-2 text-xs" :disabled="busy" @click="emit('openChat')">
        <MessageSquare class="mr-1 h-3.5 w-3.5" />
        {{ t("workspace.sr.chat") }}
      </Button>
    </div>
    <p class="text-[11px] text-muted-foreground">{{ t("workspace.sr.hubHint") }}</p>
    <div class="space-y-2">
      <button
        v-for="link in orderedLinks"
        :key="link.id"
        type="button"
        class="flex w-full items-center gap-2 rounded-md border px-3 py-2 text-left hover:bg-muted/50"
        @click="emit('openBlock', link.id)"
      >
        <span class="min-w-0 flex-1 truncate font-medium">{{ link.title }}</span>
      </button>
    </div>
  </div>
</template>
