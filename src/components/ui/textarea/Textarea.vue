<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  class?: HTMLAttributes["class"];
  modelValue?: string;
  placeholder?: string;
  disabled?: boolean;
  rows?: number | string;
}>();

const emit = defineEmits<{ "update:modelValue": [string] }>();
</script>

<template>
  <textarea
    :value="modelValue"
    :placeholder="placeholder"
    :disabled="disabled"
    :rows="rows"
    :class="
      cn(
        'flex min-h-[80px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50',
        props.class,
      )
    "
    @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
  />
</template>
