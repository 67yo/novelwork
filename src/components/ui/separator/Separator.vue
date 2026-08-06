<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";
const props = withDefaults(defineProps<{ class?: HTMLAttributes["class"]; orientation?: "horizontal" | "vertical" }>(), {
  orientation: "horizontal",
});
</script>
<template>
  <div
    :class="
      cn(
        'shrink-0 bg-border',
        props.orientation === 'horizontal' ? 'h-px w-full' : 'h-full w-px',
        props.class,
      )
    "
  />
</template>
