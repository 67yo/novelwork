<script setup lang="ts">
import { computed, inject } from "vue";
import { MessageSquare } from "@lucide/vue";
import { useI18n } from "@/i18n";
import { worldviewFanTitleKey } from "@/lib/worldview";
import { Button } from "@/components/ui/button";

const props = defineProps<{ slot: string }>();

const { t } = useI18n();
const openChat = inject<((slot: string) => void) | undefined>(
  "novework.openWorldviewSlotChat",
  undefined,
);

const title = computed(() => {
  const key = worldviewFanTitleKey(props.slot);
  return key ? t(key) : "";
});

function onChat() {
  openChat?.(props.slot);
}
</script>

<template>
  <div class="flex items-start justify-between gap-2">
    <div class="min-w-0">
      <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.knowledgeLabel") }}</label>
      <div class="flex min-h-8 items-center text-sm font-semibold leading-tight">{{ title }}</div>
    </div>
    <Button
      v-if="openChat"
      type="button"
      size="sm"
      variant="outline"
      class="h-7 shrink-0 px-2 text-xs"
      @click="onChat"
    >
      <MessageSquare class="mr-1 h-3.5 w-3.5" />
      {{ t("workspace.wv.chatFillCard") }}
    </Button>
  </div>
</template>
