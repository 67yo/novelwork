/** 历史文化：宗教 / 重大事件子卡 + 主卡固定项（slot=wv_history_culture） */
export type WorldReligion = {
  name: string;
  core_belief: string;
  followers_scope: string;
};

export type MajorEvent = {
  title: string;
  event: string;
  long_term_impact: string;
};

export type HistoryCultureData = {
  premise: string;
  customs: string;
  economy: string;
  /** 兼容旧内嵌；迁移后由子卡承载 */
  religions: WorldReligion[];
  daily_slices: string;
  major_events: MajorEvent[];
};

export const RELIGION_SLOT = "wv_religion";
export const MAJOR_EVENT_SLOT = "wv_major_event";

export function emptyReligion(): WorldReligion {
  return { name: "", core_belief: "", followers_scope: "" };
}

export function emptyMajorEvent(): MajorEvent {
  return { title: "", event: "", long_term_impact: "" };
}

export function emptyHistoryCulture(): HistoryCultureData {
  return {
    premise: "",
    customs: "",
    economy: "",
    religions: [],
    daily_slices: "",
    major_events: [],
  };
}

export function normalizeReligion(raw: Partial<WorldReligion> | null | undefined): WorldReligion {
  return {
    name: raw?.name ?? "",
    core_belief: raw?.core_belief ?? "",
    followers_scope: raw?.followers_scope ?? "",
  };
}

export function normalizeMajorEvent(raw: Partial<MajorEvent> | null | undefined): MajorEvent {
  return {
    title: raw?.title ?? "",
    event: raw?.event ?? "",
    long_term_impact: raw?.long_term_impact ?? "",
  };
}

export function normalizeHistoryCulture(
  raw: Partial<HistoryCultureData> | null | undefined,
): HistoryCultureData {
  const d = emptyHistoryCulture();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.customs = raw.customs ?? "";
  d.economy = raw.economy ?? "";
  d.daily_slices = raw.daily_slices ?? "";
  d.religions = Array.isArray(raw.religions)
    ? raw.religions.map((r) => normalizeReligion(r))
    : [];
  d.major_events = Array.isArray(raw.major_events)
    ? raw.major_events.map((e) => normalizeMajorEvent(e))
    : [];
  return d;
}

export function religionHasContent(r: WorldReligion): boolean {
  return !!(r.name.trim() || r.core_belief.trim() || r.followers_scope.trim());
}

export function majorEventHasContent(e: MajorEvent): boolean {
  return !!(e.title.trim() || e.event.trim() || e.long_term_impact.trim());
}

function formatReligionBlock(r: WorldReligion, fallbackIndex: number): string[] {
  const lines: string[] = [];
  const title = r.name.trim() || String(fallbackIndex);
  lines.push(`### ${title}`);
  if (r.core_belief.trim()) lines.push(`- 核心信念：${r.core_belief.trim()}`);
  if (r.followers_scope.trim()) lines.push(`- 追随者范围：${r.followers_scope.trim()}`);
  lines.push("");
  return lines;
}

function formatMajorEventBlock(e: MajorEvent, fallbackIndex: number): string[] {
  const lines: string[] = [];
  const title = e.title.trim() || String(fallbackIndex);
  lines.push(`### ${title}`);
  if (e.event.trim()) lines.push(`- 事件：${e.event.trim()}`);
  if (e.long_term_impact.trim()) lines.push(`- 长期影响：${e.long_term_impact.trim()}`);
  lines.push("");
  return lines;
}

export function formatHistoryCultureExtracted(
  d: HistoryCultureData,
  religionsOverride?: WorldReligion[],
  eventsOverride?: MajorEvent[],
): string {
  const lines: string[] = [];
  const premise = d.premise.trim();
  if (premise) {
    lines.push("## 一句话立意", premise, "");
  }
  const customs = d.customs.trim();
  if (customs) {
    lines.push("## 风俗习惯", customs, "");
  }
  const economy = d.economy.trim();
  if (economy) {
    lines.push("## 经济体系", economy, "");
  }
  const religions = (religionsOverride ?? d.religions).filter(religionHasContent);
  if (religions.length) {
    lines.push("## 宗教");
    religions.forEach((r, i) => lines.push(...formatReligionBlock(r, i + 1)));
  }
  const slices = d.daily_slices.trim();
  if (slices) {
    lines.push("## 日常切片", slices, "");
  }
  const events = (eventsOverride ?? d.major_events).filter(majorEventHasContent);
  if (events.length) {
    lines.push("## 重大事件");
    events.forEach((e, i) => lines.push(...formatMajorEventBlock(e, i + 1)));
  }
  return lines.join("\n").trim();
}

export function formatWorldReligionExtracted(r: WorldReligion): string {
  if (!religionHasContent(r)) return "";
  return formatReligionBlock(r, 1).join("\n").trim();
}

export function formatMajorEventExtracted(e: MajorEvent): string {
  if (!majorEventHasContent(e)) return "";
  return formatMajorEventBlock(e, 1).join("\n").trim();
}

export function parseWorldReligionJson(text: string): WorldReligion | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const r = normalizeReligion({
      name: String(o.name ?? o.名称 ?? "").trim(),
      core_belief: String(o.core_belief ?? o.核心信念 ?? "").trim(),
      followers_scope: String(o.followers_scope ?? o.追随者范围 ?? "").trim(),
    });
    return religionHasContent(r) ? r : null;
  } catch {
    return null;
  }
}

export function parseMajorEventJson(text: string): MajorEvent | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const e = normalizeMajorEvent({
      title: String(o.title ?? o.标题 ?? "").trim(),
      event: String(o.event ?? o.事件 ?? "").trim(),
      long_term_impact: String(o.long_term_impact ?? o.长期影响 ?? "").trim(),
    });
    return majorEventHasContent(e) ? e : null;
  } catch {
    return null;
  }
}

export function isReligionSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === RELIGION_SLOT;
}

export function isMajorEventSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === MAJOR_EVENT_SLOT;
}

export function isHistoryCultureChildSlot(slot: string | undefined | null): boolean {
  return isReligionSlot(slot) || isMajorEventSlot(slot);
}
