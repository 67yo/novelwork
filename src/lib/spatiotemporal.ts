/** 时空地理：关键地点 — 可存树上地点卡 knowledge.key_location */
export type KeyLocation = {
  name: string;
  features: string;
  terrain: string;
  faction: string;
};

/** 时空地理固定项（地点改挂子卡后 locations 仅兼容旧数据 / 迁移） */
export type SpatiotemporalData = {
  premise: string;
  era: string;
  ecology: string;
  world_pattern: string;
  locations: KeyLocation[];
  atmosphere: string;
};

export const LOCATION_SLOT = "wv_location";

export function emptyLocation(): KeyLocation {
  return { name: "", features: "", terrain: "", faction: "" };
}

export function emptySpatiotemporal(): SpatiotemporalData {
  return {
    premise: "",
    era: "",
    ecology: "",
    world_pattern: "",
    locations: [],
    atmosphere: "",
  };
}

export function normalizeLocation(raw: Partial<KeyLocation> | null | undefined): KeyLocation {
  return {
    name: raw?.name ?? "",
    features: raw?.features ?? "",
    terrain: raw?.terrain ?? "",
    faction: raw?.faction ?? "",
  };
}

/** 规范化：地点默认可空（由子卡承载） */
export function normalizeSpatiotemporal(
  raw: Partial<SpatiotemporalData> | null | undefined,
): SpatiotemporalData {
  const d = emptySpatiotemporal();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.era = raw.era ?? "";
  d.ecology = raw.ecology ?? "";
  d.world_pattern = raw.world_pattern ?? "";
  d.atmosphere = raw.atmosphere ?? "";
  d.locations = Array.isArray(raw.locations)
    ? raw.locations.map((l) => normalizeLocation(l))
    : [];
  return d;
}

export function locationHasContent(l: KeyLocation): boolean {
  return !!(
    l.name.trim() ||
    l.features.trim() ||
    l.terrain.trim() ||
    l.faction.trim()
  );
}

export function formatLocationBlock(l: KeyLocation): string[] {
  const lines: string[] = [];
  lines.push(`### ${l.name.trim() || "未命名"}`);
  if (l.features.trim()) lines.push(`- 特征：${l.features.trim()}`);
  if (l.terrain.trim()) lines.push(`- 地貌：${l.terrain.trim()}`);
  if (l.faction.trim()) lines.push(`- 控制势力：${l.faction.trim()}`);
  lines.push("");
  return lines;
}

/** 写入 knowledge.extracted；locationsOverride 优先（来自子卡） */
export function formatSpatiotemporalExtracted(
  d: SpatiotemporalData,
  locationsOverride?: KeyLocation[],
): string {
  const lines: string[] = [];
  const push = (title: string, body: string) => {
    const t = body.trim();
    if (!t) return;
    lines.push(`## ${title}`, t, "");
  };
  push("一句话立意", d.premise);
  push("时代背景", d.era);
  push("生态", d.ecology);
  push("世界格局", d.world_pattern);
  const locs = (locationsOverride ?? d.locations).filter(locationHasContent);
  if (locs.length) {
    lines.push("## 关键地点");
    locs.forEach((l) => lines.push(...formatLocationBlock(l)));
  }
  push("环境质感", d.atmosphere);
  return lines.join("\n").trim();
}

export function formatKeyLocationExtracted(l: KeyLocation): string {
  return formatLocationBlock(l).join("\n").trim();
}

/** 从模型输出里抠关键地点 JSON 数组 */
export function parseKeyLocationsJson(text: string): KeyLocation[] {
  const s = text.trim();
  const start = s.indexOf("[");
  const end = s.lastIndexOf("]");
  if (start < 0 || end <= start) return [];
  try {
    const arr = JSON.parse(s.slice(start, end + 1)) as unknown;
    if (!Array.isArray(arr)) return [];
    return arr
      .map((item) => {
        if (!item || typeof item !== "object") return null;
        const o = item as Record<string, unknown>;
        return normalizeLocation({
          name: String(o.name ?? o.名称 ?? ""),
          features: String(o.features ?? o.特征 ?? ""),
          terrain: String(o.terrain ?? o.地貌 ?? ""),
          faction: String(o.faction ?? o.控制势力 ?? o.势力 ?? ""),
        });
      })
      .filter((l): l is KeyLocation => !!l && locationHasContent(l));
  } catch {
    return [];
  }
}

/** 从模型输出里抠单条地点 JSON 对象 */
export function parseKeyLocationJson(text: string): KeyLocation | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const l = normalizeLocation({
      name: String(o.name ?? o.名称 ?? ""),
      features: String(o.features ?? o.特征 ?? ""),
      terrain: String(o.terrain ?? o.地貌 ?? ""),
      faction: String(o.faction ?? o.控制势力 ?? o.势力 ?? ""),
    });
    return locationHasContent(l) ? l : null;
  } catch {
    return null;
  }
}

export function isLocationSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === LOCATION_SLOT;
}
