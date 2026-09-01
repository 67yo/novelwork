<script setup lang="ts">
import type { StoryConnection } from "@/lib/flowCanvas";

const props = defineProps<{
  data: StoryConnection;
  start?: unknown;
  end?: unknown;
  path?: string;
}>();

function onClick(ev: MouseEvent) {
  ev.stopPropagation();
  props.data.onClick?.();
}

function onDblClick(ev: MouseEvent) {
  ev.stopPropagation();
  props.data.onDblClick?.();
}
</script>

<template>
  <svg class="story-conn" data-testid="connection">
    <path
      class="story-conn-hit"
      :d="path"
      fill="none"
      @click="onClick"
      @dblclick="onDblClick"
    />
    <path
      class="story-conn-line"
      :d="path"
      fill="none"
      :stroke="data.stroke || '#3d6b4f'"
    />
    <text
      v-if="data.edgeLabel"
      class="story-conn-label"
      :x="0"
      :y="0"
    >{{ data.edgeLabel }}</text>
  </svg>
</template>

<style scoped>
.story-conn {
  overflow: visible !important;
  pointer-events: none;
  position: absolute;
  left: 0;
  top: 0;
  width: 1px;
  height: 1px;
}
.story-conn-hit {
  pointer-events: stroke;
  cursor: pointer;
  stroke: transparent;
  stroke-width: 12;
}
.story-conn-line {
  pointer-events: none;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0.88;
}
.story-conn:hover .story-conn-line {
  stroke-width: 2.5;
  opacity: 1;
}
.story-conn-label {
  display: none;
}
</style>
