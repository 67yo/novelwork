<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { api, type TokenUsageDay } from "@/lib/api";
import { useI18n } from "@/i18n";

const { t } = useI18n();

function todayLocal() {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

const date = ref(todayLocal());
const data = ref<TokenUsageDay | null>(null);
const err = ref("");
const busy = ref(false);

const MODEL_COLORS = [
  "#3d6b4f",
  "#d97706",
  "#0284c7",
  "#7c3aed",
  "#dc2626",
  "#0d9488",
  "#ca8a04",
  "#db2777",
];

function colorFor(model: string, models: string[]) {
  const i = Math.max(0, models.indexOf(model));
  return MODEL_COLORS[i % MODEL_COLORS.length];
}

async function load() {
  busy.value = true;
  err.value = "";
  try {
    data.value = await api.getTokenUsage(date.value);
  } catch (e) {
    err.value = String(e);
    data.value = null;
  } finally {
    busy.value = false;
  }
}

onMounted(load);
watch(date, load);

const hours = computed(() => Array.from({ length: 24 }, (_, i) => i));

const models = computed(() => data.value?.models ?? []);

/** hour -> model -> tokens */
const matrix = computed(() => {
  const m = new Map<number, Map<string, number>>();
  for (const h of hours.value) m.set(h, new Map());
  for (const row of data.value?.rows ?? []) {
    const by = m.get(row.hour) ?? new Map();
    by.set(row.model, (by.get(row.model) ?? 0) + row.total_tokens);
    m.set(row.hour, by);
  }
  return m;
});

const maxStack = computed(() => {
  let max = 0;
  for (const by of matrix.value.values()) {
    let sum = 0;
    for (const v of by.values()) sum += v;
    if (sum > max) max = sum;
  }
  return Math.max(max, 1);
});

const dayTotal = computed(() =>
  (data.value?.rows ?? []).reduce((a, r) => a + r.total_tokens, 0),
);

const W = 720;
const H = 320;
const PAD = { t: 16, r: 12, b: 36, l: 48 };
const innerW = W - PAD.l - PAD.r;
const innerH = H - PAD.t - PAD.b;
const barGap = 2;
const barW = innerW / 24 - barGap;

type Seg = { model: string; y: number; h: number; tokens: number; color: string };

const bars = computed(() => {
  const out: { hour: number; x: number; segs: Seg[] }[] = [];
  for (const hour of hours.value) {
    const by = matrix.value.get(hour) ?? new Map();
    let y = PAD.t + innerH;
    const segs: Seg[] = [];
    for (const model of models.value) {
      const tokens = by.get(model) ?? 0;
      if (!tokens) continue;
      const h = (tokens / maxStack.value) * innerH;
      y -= h;
      segs.push({
        model,
        y,
        h: Math.max(h, tokens > 0 ? 1 : 0),
        tokens,
        color: colorFor(model, models.value),
      });
    }
    out.push({ hour, x: PAD.l + hour * (barW + barGap), segs });
  }
  return out;
});

const yTicks = computed(() => {
  const top = maxStack.value;
  const steps = 4;
  return Array.from({ length: steps + 1 }, (_, i) => {
    const v = Math.round((top * i) / steps);
    const y = PAD.t + innerH - (v / top) * innerH;
    return { v, y };
  });
});

const tip = ref<{ x: number; y: number; text: string } | null>(null);

function onSegEnter(ev: MouseEvent, hour: number, seg: Seg) {
  tip.value = {
    x: ev.clientX,
    y: ev.clientY,
    text: `${String(hour).padStart(2, "0")}:00 · ${seg.model} · ${seg.tokens.toLocaleString()} tokens`,
  };
}
function onSegLeave() {
  tip.value = null;
}
</script>

<template>
  <div class="mx-auto max-w-4xl space-y-6 p-6">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">{{ t("stats.title") }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">{{ t("stats.subtitle") }}</p>
      </div>
      <div class="flex items-center gap-2">
        <label class="text-sm text-muted-foreground" for="stats-date">{{ t("stats.date") }}</label>
        <input
          id="stats-date"
          v-model="date"
          type="date"
          class="flex h-9 rounded-md border border-input bg-background px-3 text-sm"
        />
      </div>
    </div>

    <p v-if="err" class="text-sm text-destructive">{{ err }}</p>
    <p v-if="busy" class="text-sm text-muted-foreground">{{ t("stats.loading") }}</p>

    <div class="rounded-xl border bg-card p-4">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-2 text-sm">
        <span class="text-muted-foreground">
          {{ t("stats.dayTotal", { n: dayTotal.toLocaleString() }) }}
        </span>
        <div class="flex flex-wrap gap-3">
          <span
            v-for="m in models"
            :key="m"
            class="inline-flex items-center gap-1.5 text-xs"
          >
            <span class="inline-block h-2.5 w-2.5 rounded-sm" :style="{ background: colorFor(m, models) }" />
            {{ m }}
          </span>
          <span v-if="!models.length" class="text-xs text-muted-foreground">{{ t("stats.noData") }}</span>
        </div>
      </div>

      <div class="overflow-x-auto">
        <svg :viewBox="`0 0 ${W} ${H}`" class="min-w-[640px] w-full" role="img">
          <line
            :x1="PAD.l"
            :y1="PAD.t + innerH"
            :x2="PAD.l + innerW"
            :y2="PAD.t + innerH"
            stroke="currentColor"
            class="text-border"
            stroke-width="1"
          />
          <line
            :x1="PAD.l"
            :y1="PAD.t"
            :x2="PAD.l"
            :y2="PAD.t + innerH"
            stroke="currentColor"
            class="text-border"
            stroke-width="1"
          />
          <g v-for="tick in yTicks" :key="tick.v">
            <line
              :x1="PAD.l"
              :x2="PAD.l + innerW"
              :y1="tick.y"
              :y2="tick.y"
              stroke="currentColor"
              class="text-border/60"
              stroke-width="1"
              stroke-dasharray="3 4"
            />
            <text
              :x="PAD.l - 6"
              :y="tick.y + 3"
              text-anchor="end"
              class="fill-muted-foreground"
              font-size="10"
            >
              {{ tick.v }}
            </text>
          </g>
          <g v-for="bar in bars" :key="bar.hour">
            <rect
              v-for="(seg, i) in bar.segs"
              :key="`${bar.hour}-${seg.model}-${i}`"
              :x="bar.x"
              :y="seg.y"
              :width="barW"
              :height="seg.h"
              :fill="seg.color"
              class="cursor-pointer opacity-90 hover:opacity-100"
              @mouseenter="onSegEnter($event, bar.hour, seg)"
              @mouseleave="onSegLeave"
            />
            <text
              v-if="bar.hour % 2 === 0"
              :x="bar.x + barW / 2"
              :y="PAD.t + innerH + 14"
              text-anchor="middle"
              class="fill-muted-foreground"
              font-size="9"
            >
              {{ String(bar.hour).padStart(2, "0") }}
            </text>
          </g>
        </svg>
      </div>
      <p class="mt-1 text-center text-[10px] text-muted-foreground">{{ t("stats.xAxis") }}</p>
    </div>

    <div
      v-if="tip"
      class="pointer-events-none fixed z-50 rounded-md border bg-popover px-2 py-1 text-xs shadow"
      :style="{ left: tip.x + 12 + 'px', top: tip.y + 12 + 'px' }"
    >
      {{ tip.text }}
    </div>
  </div>
</template>
