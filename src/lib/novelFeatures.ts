/** Novel positioning tags (create form + root panel). Preset ids are stable; custom = free text. */

export type NovelFeatures = {
  genres: string[];
  core_play: string[];
  styles: string[];
  relationships: string[];
  audiences: string[];
};

export type FeatureGroupKey = keyof NovelFeatures;

export type FeatureGroupDef = {
  key: FeatureGroupKey;
  /** i18n: novels.feat.group.{key} */
  allowCustom: boolean;
  options: readonly string[];
};

export const FEATURE_GROUPS: readonly FeatureGroupDef[] = [
  {
    key: "genres",
    allowCustom: true,
    options: [
      "urban",
      "xuanhuan",
      "xianxia",
      "scifi",
      "western_fantasy",
      "history",
      "military",
      "mystery",
      "supernatural",
      "game",
      "apocalypse",
    ],
  },
  {
    key: "core_play",
    allowCustom: true,
    options: [
      "system",
      "rebirth",
      "transmigration",
      "infinite_flow",
      "farming",
      "lord_building",
      "tycoon",
      "cautious",
      "academy",
      "rule_horror",
      "dungeon",
      "many_children",
    ],
  },
  {
    key: "styles",
    allowCustom: true,
    options: [
      "cool",
      "funny",
      "hotblood",
      "dark",
      "light",
      "healing",
      "horror",
      "realistic",
      "ensemble",
      "epic",
    ],
  },
  {
    key: "relationships",
    allowCustom: false,
    options: ["single_heroine", "multi_heroine", "no_cp", "dual_heroine", "harem", "pure_love"],
  },
  {
    key: "audiences",
    allowCustom: false,
    options: ["male", "female"],
  },
] as const;

export function emptyNovelFeatures(): NovelFeatures {
  return {
    genres: [],
    core_play: [],
    styles: [],
    relationships: [],
    audiences: [],
  };
}

export function normalizeNovelFeatures(raw: unknown): NovelFeatures {
  const base = emptyNovelFeatures();
  if (!raw || typeof raw !== "object") return base;
  const o = raw as Record<string, unknown>;
  for (const g of FEATURE_GROUPS) {
    const v = o[g.key];
    if (!Array.isArray(v)) continue;
    const preset = new Set(g.options);
    const out: string[] = [];
    const seen = new Set<string>();
    for (const item of v) {
      if (typeof item !== "string") continue;
      const s = item.trim();
      if (!s || seen.has(s)) continue;
      if (preset.has(s) || g.allowCustom) {
        seen.add(s);
        out.push(s);
      }
    }
    base[g.key] = out;
  }
  return base;
}

export function toggleFeatureValue(
  features: NovelFeatures,
  group: FeatureGroupKey,
  value: string,
): NovelFeatures {
  const cur = features[group];
  const next = cur.includes(value) ? cur.filter((x) => x !== value) : [...cur, value];
  return { ...features, [group]: next };
}

export function addCustomFeature(
  features: NovelFeatures,
  group: FeatureGroupKey,
  raw: string,
): NovelFeatures {
  const def = FEATURE_GROUPS.find((g) => g.key === group);
  if (!def?.allowCustom) return features;
  const s = raw.trim();
  if (!s) return features;
  if (features[group].includes(s)) return features;
  return { ...features, [group]: [...features[group], s] };
}

export function isPresetOption(group: FeatureGroupKey, value: string): boolean {
  const def = FEATURE_GROUPS.find((g) => g.key === group);
  return !!def?.options.includes(value);
}

export function customValues(features: NovelFeatures, group: FeatureGroupKey): string[] {
  const def = FEATURE_GROUPS.find((g) => g.key === group);
  if (!def) return [];
  const preset = new Set(def.options);
  return features[group].filter((v) => !preset.has(v));
}
