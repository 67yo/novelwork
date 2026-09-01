<script setup lang="ts">
defineProps<{ data?: { name?: string } }>();
</script>

<template>
  <div class="story-socket" :data-socket="data?.name || 'port'" />
</template>

<style scoped>
.story-socket {
  box-sizing: border-box;
  height: 10px;
  width: 10px;
  border-radius: 999px;
  border: 2px solid currentColor;
  background: currentColor;
  cursor: pointer;
}
</style>
