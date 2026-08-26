/** 社会权力：种族 */
export type WorldRace = {
  name: string;
  features: string;
  population: string;
  social_status: string;
};

/** 社会权力：主要势力 */
export type MajorFaction = {
  name: string;
  faction_type: string;
  goal: string;
  means: string;
  power_base: string;
};

/** 社会权力固定项（种族/势力改挂子卡后内嵌数组仅兼容迁移） */
export type SocialPowerData = {
  premise: string;
  races: WorldRace[];
  factions: MajorFaction[];
  class_structure: string;
  political_system: string;
  power_visibility: string;
};

export const RACE_SLOT = "wv_race";
export const FACTION_SLOT = "wv_faction";

export function emptyRace(): WorldRace {
  return { name: "", features: "", population: "", social_status: "" };
}

export function emptyFaction(): MajorFaction {
  return { name: "", faction_type: "", goal: "", means: "", power_base: "" };
}

export function emptySocialPower(): SocialPowerData {
  return {
    premise: "",
    races: [],
    factions: [],
    class_structure: "",
    political_system: "",
    power_visibility: "",
  };
}

export function normalizeRace(raw: Partial<WorldRace> | null | undefined): WorldRace {
  return {
    name: raw?.name ?? "",
    features: raw?.features ?? "",
    population: raw?.population ?? "",
    social_status: raw?.social_status ?? "",
  };
}

export function normalizeFaction(raw: Partial<MajorFaction> | null | undefined): MajorFaction {
  return {
    name: raw?.name ?? "",
    faction_type: raw?.faction_type ?? "",
    goal: raw?.goal ?? "",
    means: raw?.means ?? "",
    power_base: raw?.power_base ?? "",
  };
}

export function normalizeSocialPower(
  raw: Partial<SocialPowerData> | null | undefined,
): SocialPowerData {
  const d = emptySocialPower();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.class_structure = raw.class_structure ?? "";
  d.political_system = raw.political_system ?? "";
  d.power_visibility = raw.power_visibility ?? "";
  d.races = Array.isArray(raw.races) ? raw.races.map((r) => normalizeRace(r)) : [];
  d.factions = Array.isArray(raw.factions)
    ? raw.factions.map((f) => normalizeFaction(f))
    : [];
  return d;
}

export function raceHasContent(r: WorldRace): boolean {
  return !!(
    r.name.trim() ||
    r.features.trim() ||
    r.population.trim() ||
    r.social_status.trim()
  );
}

export function factionHasContent(f: MajorFaction): boolean {
  return !!(
    f.name.trim() ||
    f.faction_type.trim() ||
    f.goal.trim() ||
    f.means.trim() ||
    f.power_base.trim()
  );
}

export function formatRaceBlock(r: WorldRace): string[] {
  const lines: string[] = [];
  lines.push(`### ${r.name.trim() || "未命名"}`);
  if (r.features.trim()) lines.push(`- 特征：${r.features.trim()}`);
  if (r.population.trim()) lines.push(`- 人口：${r.population.trim()}`);
  if (r.social_status.trim()) lines.push(`- 社会地位：${r.social_status.trim()}`);
  lines.push("");
  return lines;
}

export function formatFactionBlock(f: MajorFaction): string[] {
  const lines: string[] = [];
  const title = f.name.trim() || f.faction_type.trim() || "未命名";
  lines.push(`### ${title}`);
  if (f.faction_type.trim()) lines.push(`- 类型：${f.faction_type.trim()}`);
  if (f.goal.trim()) lines.push(`- 目标：${f.goal.trim()}`);
  if (f.means.trim()) lines.push(`- 手段：${f.means.trim()}`);
  if (f.power_base.trim()) lines.push(`- 权力基础：${f.power_base.trim()}`);
  lines.push("");
  return lines;
}

export function formatSocialPowerExtracted(
  d: SocialPowerData,
  racesOverride?: WorldRace[],
  factionsOverride?: MajorFaction[],
): string {
  const lines: string[] = [];
  const push = (title: string, body: string) => {
    const t = body.trim();
    if (!t) return;
    lines.push(`## ${title}`, t, "");
  };
  push("一句话立意", d.premise);
  const races = (racesOverride ?? d.races).filter(raceHasContent);
  if (races.length) {
    lines.push("## 种族");
    races.forEach((r) => lines.push(...formatRaceBlock(r)));
  }
  const factions = (factionsOverride ?? d.factions).filter(factionHasContent);
  if (factions.length) {
    lines.push("## 主要势力");
    factions.forEach((f) => lines.push(...formatFactionBlock(f)));
  }
  push("阶层结构", d.class_structure);
  push("政治体制", d.political_system);
  push("权力可见性", d.power_visibility);
  return lines.join("\n").trim();
}

export function formatWorldRaceExtracted(r: WorldRace): string {
  return formatRaceBlock(r).join("\n").trim();
}

export function formatMajorFactionExtracted(f: MajorFaction): string {
  return formatFactionBlock(f).join("\n").trim();
}

export function parseWorldRaceJson(text: string): WorldRace | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const r = normalizeRace({
      name: String(o.name ?? o.名称 ?? ""),
      features: String(o.features ?? o.特征 ?? ""),
      population: String(o.population ?? o.人口 ?? ""),
      social_status: String(o.social_status ?? o.社会地位 ?? ""),
    });
    return raceHasContent(r) ? r : null;
  } catch {
    return null;
  }
}

export function parseMajorFactionJson(text: string): MajorFaction | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const f = normalizeFaction({
      name: String(o.name ?? o.名称 ?? ""),
      faction_type: String(o.faction_type ?? o.type ?? o.类型 ?? ""),
      goal: String(o.goal ?? o.目标 ?? ""),
      means: String(o.means ?? o.手段 ?? ""),
      power_base: String(o.power_base ?? o.权力基础 ?? ""),
    });
    return factionHasContent(f) ? f : null;
  } catch {
    return null;
  }
}

export function isRaceSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === RACE_SLOT;
}

export function isFactionSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === FACTION_SLOT;
}
