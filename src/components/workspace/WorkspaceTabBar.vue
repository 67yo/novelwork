<script setup lang="ts">
import { BookOpen, GitBranch, Globe, Library, ListTree, Network, User } from "@lucide/vue";
import { useI18n } from "@/i18n";

export type WorkspaceTabId =
  | "book"
  | "worldview"
  | "characters"
  | "story-rules"
  | "manuscript"
  | "plots"
  | "tree";

defineProps<{
  modelValue: WorkspaceTabId;
}>();
const emit = defineEmits<{
  "update:modelValue": [id: WorkspaceTabId];
}>();

const { t } = useI18n();
const tabs: {
  id: WorkspaceTabId;
  labelKey:
    | "workspace.tabBook"
    | "workspace.tabWorldview"
    | "workspace.tabCharacters"
    | "workspace.tabStoryRules"
    | "workspace.tabManuscript"
    | "workspace.tabPlots"
    | "workspace.tabTree";
  icon: typeof BookOpen;
}[] = [
  { id: "book", labelKey: "workspace.tabBook", icon: Library },
  { id: "worldview", labelKey: "workspace.tabWorldview", icon: Globe },
  { id: "characters", labelKey: "workspace.tabCharacters", icon: User },
  { id: "story-rules", labelKey: "workspace.tabStoryRules", icon: ListTree },
  { id: "manuscript", labelKey: "workspace.tabManuscript", icon: BookOpen },
  { id: "plots", labelKey: "workspace.tabPlots", icon: GitBranch },
  { id: "tree", labelKey: "workspace.tabTree", icon: Network },
];
</script>

<template>
  <div class="flex shrink-0 items-center gap-0.5 overflow-x-auto border-b px-2 py-1">
    <button
      v-for="tab in tabs"
      :key="tab.id"
      type="button"
      class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs"
      :class="
        modelValue === tab.id
          ? 'bg-primary/10 font-medium text-primary'
          : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground'
      "
      @click="emit('update:modelValue', tab.id)"
    >
      <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
      {{ t(tab.labelKey) }}
    </button>
  </div>
</template>
