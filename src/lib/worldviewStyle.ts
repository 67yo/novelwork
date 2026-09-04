/** 世界观扇形六卡画布配色（与子卡 wv_race / wv_faction 等区分） */
export type WorldviewFanVisual = {
  shell: string;
  icon: string;
  handle: string;
  glow: string;
  badge: string;
};

export const WORLDVIEW_FAN_VISUAL: Record<string, WorldviewFanVisual> = {
  wv_core_laws: {
    shell: "border-indigo-700/35 bg-[oklch(0.96_0.035_280)] dark:bg-[oklch(0.3_0.04_280)] min-w-[160px]",
    icon: "text-indigo-700 dark:text-indigo-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-indigo-600 !bg-indigo-500",
    glow: "story-node-glow story-node-glow--indigo",
    badge: "text-indigo-800",
  },
  wv_spatiotemporal: {
    shell: "border-teal-700/35 bg-[oklch(0.96_0.035_175)] dark:bg-[oklch(0.3_0.04_175)] min-w-[160px]",
    icon: "text-teal-700 dark:text-teal-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-teal-700 !bg-teal-600",
    glow: "story-node-glow story-node-glow--teal",
    badge: "text-teal-800",
  },
  wv_social_power: {
    shell: "border-violet-700/35 bg-[oklch(0.96_0.035_300)] dark:bg-[oklch(0.3_0.04_300)] min-w-[160px]",
    icon: "text-violet-700 dark:text-violet-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-violet-600 !bg-violet-500",
    glow: "story-node-glow story-node-glow--violet",
    badge: "text-violet-800",
  },
  wv_history_culture: {
    shell: "border-amber-700/35 bg-[oklch(0.97_0.04_85)] dark:bg-[oklch(0.3_0.04_85)] min-w-[160px]",
    icon: "text-amber-700 dark:text-amber-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-amber-600 !bg-amber-500",
    glow: "story-node-glow story-node-glow--amber",
    badge: "text-amber-800",
  },
  wv_existence: {
    shell: "border-emerald-700/35 bg-[oklch(0.96_0.035_155)] dark:bg-[oklch(0.3_0.04_155)] min-w-[160px]",
    icon: "text-emerald-700 dark:text-emerald-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-emerald-600 !bg-emerald-500",
    glow: "story-node-glow story-node-glow--emerald",
    badge: "text-emerald-800",
  },
  wv_info_flow: {
    shell: "border-sky-700/35 bg-[oklch(0.95_0.035_220)] dark:bg-[oklch(0.3_0.04_220)] min-w-[160px]",
    icon: "text-sky-700 dark:text-sky-300",
    handle: "!h-2.5 !w-2.5 !border-2 !border-sky-600 !bg-sky-500",
    glow: "story-node-glow story-node-glow--sky",
    badge: "text-sky-800",
  },
};

export function worldviewFanVisual(slot: string): WorldviewFanVisual | null {
  return WORLDVIEW_FAN_VISUAL[slot] ?? null;
}
