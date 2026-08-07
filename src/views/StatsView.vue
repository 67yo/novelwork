<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { api, type TokenUsageDay, type TokenUsageMonth } from "@/lib/api";
import { useI18n } from "@/i18n";

const { t } = useI18n();

function todayLocal() {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function monthLocal() {
  return todayLocal().slice(0, 7);
}

/** 近 36 个月（含当月）；WebView 里 type=month 常变手输，改用下拉。 */
function buildMonthOptions(count = 36) {
  const out: string[] = [];
  const d = new Date();
  d.setDate(1);
  for (let i = 0; i < count; i++) {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    out.push(`${y}-${m}`);
    d.setMonth(d.getMonth() - 1);
  }
  return out;
}

const date = ref(todayLocal());
const data = ref<TokenUsageDay | null>(null);
const monthOptions = buildMonthOptions();
const month = ref(monthLocal());
const monthData = ref<TokenUsageMonth | null>(null);
const err = ref("");
const busy = ref(false);
const monthBusy = ref(false);

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

function fmtTok(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return n.toLocaleString();
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

async function loadMonth() {
  monthBusy.value = true;
  try {
    monthData.value = await api.getTokenUsageMonth(month.value);
  } catch (e) {
    err.value = String(e);
    monthData.value = null;
  } finally {
    monthBusy.value = false;
  }
}

onMounted(() => {
  void load();
  void loadMonth();
});
watch(date, load);
watch(month, loadMonth);

const hours = computed(() => Array.from({ length: 24 }, (_, i) => i));

const models = computed(() => data.value?.models ?? []);
const novels = computed(() => data.value?.novels ?? []);

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

const hourTotals = computed(() => {
  const totals = new Map<number, number>();
  for (const [h, by] of matrix.value) {
    let sum = 0;
    for (const v of by.values()) sum += v;
    totals.set(h, sum);
  }
  return totals;
});

const maxStack = computed(() => {
  let max = 0;
  for (const v of hourTotals.value.values()) if (v > max) max = v;
  return Math.max(max, 1);
});

const dayTotal = computed(() =>
  (data.value?.rows ?? []).reduce((a, r) => a + r.total_tokens, 0),
);

const W = 720;
const H = 340;
const PAD = { t: 28, r: 12, b: 36, l: 48 };
const innerW = W - PAD.l - PAD.r;
const innerH = H - PAD.t - PAD.b;
const barGap = 2;
const barW = innerW / 24 - barGap;

type Seg = { model: string; y: number; h: number; tokens: number; color: string };

const bars = computed(() => {
  const out: { hour: number; x: number; total: number; topY: number; segs: Seg[] }[] = [];
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
    const total = hourTotals.value.get(hour) ?? 0;
    out.push({ hour, x: PAD.l + hour * (barW + barGap), total, topY: y, segs });
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

function onSegEnter(ev: MouseEvent, hour: number, seg: Seg, hourTotal: number) {
  tip.value = {
    x: ev.clientX,
    y: ev.clientY,
    text: t("stats.hourTip", {
      h: String(hour).padStart(2, "0"),
      hour: hourTotal.toLocaleString(),
      model: seg.model,
      n: seg.tokens.toLocaleString(),
    }),
  };
}
function onSegLeave() {
  tip.value = null;
}

const monthModels = computed(() => monthData.value?.models.map((r) => r.model) ?? []);
const monthMax = computed(() =>
  Math.max(1, ...(monthData.value?.models.map((r) => r.total_tokens) ?? [0])),
);
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
        <span class="font-medium">
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
              @mouseenter="onSegEnter($event, bar.hour, seg, bar.total)"
              @mouseleave="onSegLeave"
            />
            <text
              v-if="bar.total > 0"
              :x="bar.x + barW / 2"
              :y="bar.topY - 4"
              text-anchor="middle"
              class="fill-foreground"
              font-size="8"
            >
              {{ fmtTok(bar.total) }}
            </text>
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

    <div class="rounded-xl border bg-card p-4">
      <div class="mb-3 flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 class="text-sm font-medium">{{ t("stats.monthTitle") }}</h2>
          <p class="mt-0.5 text-xs text-muted-foreground">{{ t("stats.monthHint") }}</p>
        </div>
        <div class="flex items-center gap-2">
          <label class="text-sm text-muted-foreground" for="stats-month">{{ t("stats.month") }}</label>
          <select
            id="stats-month"
            v-model="month"
            class="flex h-9 rounded-md border border-input bg-background px-3 text-sm"
          >
            <option v-for="m in monthOptions" :key="m" :value="m">{{ m }}</option>
          </select>
        </div>
      </div>
      <p v-if="monthBusy" class="text-sm text-muted-foreground">{{ t("stats.loading") }}</p>
      <template v-else>
        <p class="mb-3 text-sm font-medium">
          {{ t("stats.monthTotal", { n: (monthData?.total_tokens ?? 0).toLocaleString() }) }}
        </p>
        <div v-if="!(monthData?.models.length)" class="text-sm text-muted-foreground">
          {{ t("stats.monthNoData") }}
        </div>
        <div v-else class="space-y-2">
          <div
            v-for="row in monthData?.models"
            :key="row.model"
            class="grid grid-cols-[minmax(0,9rem)_1fr_auto] items-center gap-2 text-sm"
          >
            <span class="truncate text-xs" :title="row.model">{{ row.model }}</span>
            <div class="h-3 overflow-hidden rounded-sm bg-muted">
              <div
                class="h-full rounded-sm"
                :style="{
                  width: `${(row.total_tokens / monthMax) * 100}%`,
                  background: colorFor(row.model, monthModels),
                }"
              />
            </div>
            <span class="tabular-nums text-xs text-muted-foreground">
              {{ row.total_tokens.toLocaleString() }}
            </span>
          </div>
        </div>
      </template>
    </div>

    <div class="rounded-xl border bg-card p-4">
      <h2 class="text-sm font-medium">{{ t("stats.novelReport") }}</h2>
      <p class="mt-0.5 text-xs text-muted-foreground">{{ t("stats.novelReportHint") }}</p>
      <div v-if="!novels.length" class="mt-3 text-sm text-muted-foreground">{{ t("stats.novelNoData") }}</div>
      <div v-else class="mt-3 overflow-x-auto">
        <table class="w-full min-w-[420px] text-left text-sm">
          <thead class="border-b text-xs text-muted-foreground">
            <tr>
              <th class="py-2 pr-3 font-medium">{{ t("stats.novelTitle") }}</th>
              <th class="py-2 pr-3 font-medium tabular-nums">{{ t("stats.novelDay") }}</th>
              <th class="py-2 font-medium tabular-nums">{{ t("stats.novelTotal") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="n in novels" :key="n.novel_id" class="border-b border-border/60 last:border-0">
              <td class="py-2 pr-3">{{ n.title }}</td>
              <td class="py-2 pr-3 tabular-nums">{{ n.day_tokens.toLocaleString() }}</td>
              <td class="py-2 tabular-nums font-medium">{{ n.total_tokens.toLocaleString() }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <div
      v-if="tip"
      class="pointer-events-none fixed z-50 max-w-xs rounded-md border bg-popover px-2 py-1 text-xs shadow"
      :style="{ left: tip.x + 12 + 'px', top: tip.y + 12 + 'px' }"
    >
      {{ tip.text }}
    </div>
  </div>
</template>
