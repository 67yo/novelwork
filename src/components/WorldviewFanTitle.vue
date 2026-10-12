<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "@/i18n";
import { worldviewFanTitleKey } from "@/lib/worldview";

const props = defineProps<{ slot: string }>();

const { t } = useI18n();

const title = computed(() => {
  const key = worldviewFanTitleKey(props.slot);
  return key ? t(key) : "";
});
</script>

<template>
  <div>
    <label class="mb-1 block text-xs text-muted-foreground">{{ t("workspace.knowledgeLabel") }}</label>
    <div class="flex min-h-8 items-center text-sm font-semibold leading-tight">{{ title }}</div>
  </div>
</template>
