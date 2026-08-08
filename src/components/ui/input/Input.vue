<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  class?: HTMLAttributes["class"];
  modelValue?: string | number | null;
  type?: string;
  placeholder?: string;
  disabled?: boolean;
  readonly?: boolean;
  min?: number | string;
  max?: number | string;
}>();

const emit = defineEmits<{ "update:modelValue": [string | number] }>();
</script>

<template>
  <input
    v-bind="$attrs"
    :type="type ?? 'text'"
    :value="modelValue ?? ''"
    :placeholder="placeholder"
    :disabled="disabled"
    :readonly="readonly"
    :min="min"
    :max="max"
    :class="
      cn(
        'flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50',
        props.class,
      )
    "
    @input="
      emit(
        'update:modelValue',
        type === 'number'
          ? Number(($event.target as HTMLInputElement).value)
          : ($event.target as HTMLInputElement).value,
      )
    "
  />
</template>
