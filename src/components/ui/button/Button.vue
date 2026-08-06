<script setup lang="ts">
import { cn } from "@/lib/utils";
import type { HTMLAttributes } from "vue";
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    variant?: "default" | "secondary" | "outline" | "ghost" | "destructive";
    size?: "default" | "sm" | "lg" | "icon";
    class?: HTMLAttributes["class"];
    disabled?: boolean;
    type?: "button" | "submit" | "reset";
  }>(),
  { variant: "default", size: "default", type: "button" },
);

const classes = computed(() =>
  cn(
    "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50",
    {
      default: "bg-primary text-primary-foreground hover:bg-primary/90",
      secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
      outline: "border border-input bg-background hover:bg-accent",
      ghost: "hover:bg-accent hover:text-accent-foreground",
      destructive: "bg-destructive text-white hover:bg-destructive/90",
    }[props.variant],
    {
      default: "h-9 px-4 py-2",
      sm: "h-8 rounded-md px-3 text-xs",
      lg: "h-10 rounded-md px-6",
      icon: "h-9 w-9",
    }[props.size],
    props.class,
  ),
);
</script>

<template>
  <button :type="type" :class="classes" :disabled="disabled">
    <slot />
  </button>
</template>
