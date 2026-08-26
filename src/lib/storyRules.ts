import type { MessageKey } from "@/i18n/messages";

/** 故事规则右侧扇形四卡（挂在 story_rules 下，不可删、连线不可断） */
export const STORY_RULES_FAN_SLOTS = [
  { slot: "sr_surface_setting", titleKey: "workspace.sr.surface" },
  { slot: "sr_story_engine", titleKey: "workspace.sr.engine" },
  { slot: "sr_fulfillment_system", titleKey: "workspace.sr.fulfillment" },
  { slot: "sr_constraint_redlines", titleKey: "workspace.sr.constraints" },
] as const;

export type StoryRulesBlockSlot = (typeof STORY_RULES_FAN_SLOTS)[number]["slot"];

const FAN_SLOT_SET = new Set<string>(STORY_RULES_FAN_SLOTS.map((s) => s.slot));

export type SurfaceSettingData = {
  premise: string;
  core_conflict: string;
  reader_promise: string;
  target_audience: string;
  tone_reference: string;
  commercial_tags: string;
  extended_premise: string;
};

export type StoryEngineData = {
  premise: string;
  bright_line: string;
  dark_line: string;
  suspense_setup: string;
  conflict_engine: string;
  external_conflict: string;
  internal_conflict: string;
  relational_conflict: string;
  progression_cycle: string;
  protagonist_dilemma: string;
};

export type FulfillmentSystemData = {
  premise: string;
  growth_path: string;
  ending_texture: string;
  payoff_syntax: string[];
  emotional_rhythm: string;
  tension_circles: string[];
};

export type ConstraintRedlinesData = {
  premise: string;
  redlines: string[];
};

export type StoryRulesBlockData =
  | SurfaceSettingData
  | StoryEngineData
  | FulfillmentSystemData
  | ConstraintRedlinesData;

export type StoryRulesFieldKind = "text" | "list";

export type StoryRulesFieldDef = {
  key: string;
  labelKey: MessageKey;
  phKey: MessageKey;
  kind: StoryRulesFieldKind;
  /** 分组标题（如「明暗双线」） */
  sectionKey?: MessageKey;
};

export const STORY_RULES_BLOCK_FIELDS: Record<StoryRulesBlockSlot, StoryRulesFieldDef[]> = {
  sr_surface_setting: [
    { key: "premise", labelKey: "workspace.sr.premise", phKey: "workspace.sr.premisePh", kind: "text" },
    {
      key: "core_conflict",
      labelKey: "workspace.sr.coreConflict",
      phKey: "workspace.sr.coreConflictPh",
      kind: "text",
    },
    {
      key: "reader_promise",
      labelKey: "workspace.sr.readerPromise",
      phKey: "workspace.sr.readerPromisePh",
      kind: "text",
    },
    {
      key: "target_audience",
      labelKey: "workspace.sr.targetAudience",
      phKey: "workspace.sr.targetAudiencePh",
      kind: "text",
    },
    {
      key: "tone_reference",
      labelKey: "workspace.sr.toneReference",
      phKey: "workspace.sr.toneReferencePh",
      kind: "text",
    },
    {
      key: "commercial_tags",
      labelKey: "workspace.sr.commercialTags",
      phKey: "workspace.sr.commercialTagsPh",
      kind: "text",
    },
    {
      key: "extended_premise",
      labelKey: "workspace.sr.extendedPremise",
      phKey: "workspace.sr.extendedPremisePh",
      kind: "text",
    },
  ],
  sr_story_engine: [
    { key: "premise", labelKey: "workspace.sr.premise", phKey: "workspace.sr.premisePh", kind: "text" },
    {
      key: "bright_line",
      labelKey: "workspace.sr.brightLine",
      phKey: "workspace.sr.brightLinePh",
      kind: "text",
      sectionKey: "workspace.sr.dualPlot",
    },
    {
      key: "dark_line",
      labelKey: "workspace.sr.darkLine",
      phKey: "workspace.sr.darkLinePh",
      kind: "text",
    },
    {
      key: "suspense_setup",
      labelKey: "workspace.sr.suspenseSetup",
      phKey: "workspace.sr.suspenseSetupPh",
      kind: "text",
    },
    {
      key: "conflict_engine",
      labelKey: "workspace.sr.conflictEngine",
      phKey: "workspace.sr.conflictEnginePh",
      kind: "text",
    },
    {
      key: "external_conflict",
      labelKey: "workspace.sr.externalConflict",
      phKey: "workspace.sr.externalConflictPh",
      kind: "text",
      sectionKey: "workspace.sr.conflictLayers",
    },
    {
      key: "internal_conflict",
      labelKey: "workspace.sr.internalConflict",
      phKey: "workspace.sr.internalConflictPh",
      kind: "text",
    },
    {
      key: "relational_conflict",
      labelKey: "workspace.sr.relationalConflict",
      phKey: "workspace.sr.relationalConflictPh",
      kind: "text",
    },
    {
      key: "progression_cycle",
      labelKey: "workspace.sr.progressionCycle",
      phKey: "workspace.sr.progressionCyclePh",
      kind: "text",
    },
    {
      key: "protagonist_dilemma",
      labelKey: "workspace.sr.protagonistDilemma",
      phKey: "workspace.sr.protagonistDilemmaPh",
      kind: "text",
    },
  ],
  sr_fulfillment_system: [
    { key: "premise", labelKey: "workspace.sr.premise", phKey: "workspace.sr.premisePh", kind: "text" },
    {
      key: "growth_path",
      labelKey: "workspace.sr.growthPath",
      phKey: "workspace.sr.growthPathPh",
      kind: "text",
    },
    {
      key: "ending_texture",
      labelKey: "workspace.sr.endingTexture",
      phKey: "workspace.sr.endingTexturePh",
      kind: "text",
    },
    {
      key: "payoff_syntax",
      labelKey: "workspace.sr.payoffSyntax",
      phKey: "workspace.sr.payoffSyntaxPh",
      kind: "list",
    },
    {
      key: "emotional_rhythm",
      labelKey: "workspace.sr.emotionalRhythm",
      phKey: "workspace.sr.emotionalRhythmPh",
      kind: "text",
    },
    {
      key: "tension_circles",
      labelKey: "workspace.sr.tensionCircles",
      phKey: "workspace.sr.tensionCirclesPh",
      kind: "list",
    },
  ],
  sr_constraint_redlines: [
    { key: "premise", labelKey: "workspace.sr.premise", phKey: "workspace.sr.premisePh", kind: "text" },
    {
      key: "redlines",
      labelKey: "workspace.sr.redlines",
      phKey: "workspace.sr.redlinesPh",
      kind: "list",
    },
  ],
};

export const STORY_RULES_BLOCK_WHOLE_PREFIX = "__story_rules_block__:";

export function storyRulesBlockWholeLabel(slot: StoryRulesBlockSlot): string {
  return `${STORY_RULES_BLOCK_WHOLE_PREFIX}${slot}`;
}

export function parseStoryRulesBlockWholeLabel(label: string): StoryRulesBlockSlot | null {
  const s = label.trim();
  if (!s.startsWith(STORY_RULES_BLOCK_WHOLE_PREFIX)) return null;
  const slot = s.slice(STORY_RULES_BLOCK_WHOLE_PREFIX.length).trim();
  return isStoryRulesFanSlot(slot) ? (slot as StoryRulesBlockSlot) : null;
}

export function isStoryRulesFanSlot(slot: string | undefined | null): boolean {
  return !!slot && FAN_SLOT_SET.has(slot);
}

export function storyRulesFanTitleKey(
  slot: string,
): (typeof STORY_RULES_FAN_SLOTS)[number]["titleKey"] | null {
  const def = STORY_RULES_FAN_SLOTS.find((s) => s.slot === slot);
  return def?.titleKey ?? null;
}

function normList(raw: unknown): string[] {
  if (!Array.isArray(raw) || !raw.length) return [""];
  return raw.map((x) => (x == null ? "" : String(x)));
}

export function emptySurfaceSetting(): SurfaceSettingData {
  return {
    premise: "",
    core_conflict: "",
    reader_promise: "",
    target_audience: "",
    tone_reference: "",
    commercial_tags: "",
    extended_premise: "",
  };
}

export function emptyStoryEngine(): StoryEngineData {
  return {
    premise: "",
    bright_line: "",
    dark_line: "",
    suspense_setup: "",
    conflict_engine: "",
    external_conflict: "",
    internal_conflict: "",
    relational_conflict: "",
    progression_cycle: "",
    protagonist_dilemma: "",
  };
}

export function emptyFulfillmentSystem(): FulfillmentSystemData {
  return {
    premise: "",
    growth_path: "",
    ending_texture: "",
    payoff_syntax: [""],
    emotional_rhythm: "",
    tension_circles: [""],
  };
}

export function emptyConstraintRedlines(): ConstraintRedlinesData {
  return { premise: "", redlines: [""] };
}

export function normalizeSurfaceSetting(
  raw: Partial<SurfaceSettingData> | null | undefined,
): SurfaceSettingData {
  const d = emptySurfaceSetting();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.core_conflict = raw.core_conflict ?? "";
  d.reader_promise = raw.reader_promise ?? "";
  d.target_audience = raw.target_audience ?? "";
  d.tone_reference = raw.tone_reference ?? "";
  d.commercial_tags = raw.commercial_tags ?? "";
  d.extended_premise = raw.extended_premise ?? "";
  return d;
}

export function normalizeStoryEngine(raw: Partial<StoryEngineData> | null | undefined): StoryEngineData {
  const d = emptyStoryEngine();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.bright_line = raw.bright_line ?? "";
  d.dark_line = raw.dark_line ?? "";
  d.suspense_setup = raw.suspense_setup ?? "";
  d.conflict_engine = raw.conflict_engine ?? "";
  d.external_conflict = raw.external_conflict ?? "";
  d.internal_conflict = raw.internal_conflict ?? "";
  d.relational_conflict = raw.relational_conflict ?? "";
  d.progression_cycle = raw.progression_cycle ?? "";
  d.protagonist_dilemma = raw.protagonist_dilemma ?? "";
  return d;
}

export function normalizeFulfillmentSystem(
  raw: Partial<FulfillmentSystemData> | null | undefined,
): FulfillmentSystemData {
  const d = emptyFulfillmentSystem();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.growth_path = raw.growth_path ?? "";
  d.ending_texture = raw.ending_texture ?? "";
  d.payoff_syntax = normList(raw.payoff_syntax);
  d.emotional_rhythm = raw.emotional_rhythm ?? "";
  d.tension_circles = normList(raw.tension_circles);
  return d;
}

export function normalizeConstraintRedlines(
  raw: Partial<ConstraintRedlinesData> | null | undefined,
): ConstraintRedlinesData {
  const d = emptyConstraintRedlines();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.redlines = normList(raw.redlines);
  return d;
}

export function normalizeStoryRulesBlock(
  slot: StoryRulesBlockSlot,
  raw: Record<string, unknown> | null | undefined,
): StoryRulesBlockData {
  if (slot === "sr_surface_setting") return normalizeSurfaceSetting(raw as SurfaceSettingData);
  if (slot === "sr_story_engine") return normalizeStoryEngine(raw as StoryEngineData);
  if (slot === "sr_fulfillment_system") return normalizeFulfillmentSystem(raw as FulfillmentSystemData);
  return normalizeConstraintRedlines(raw as ConstraintRedlinesData);
}

export function blockDataFromKnowledge(
  slot: StoryRulesBlockSlot,
  k: {
    surface_setting?: Partial<SurfaceSettingData> | null;
    story_engine?: Partial<StoryEngineData> | null;
    fulfillment_system?: Partial<FulfillmentSystemData> | null;
    constraint_redlines?: Partial<ConstraintRedlinesData> | null;
  } | null | undefined,
): StoryRulesBlockData {
  if (slot === "sr_surface_setting") return normalizeSurfaceSetting(k?.surface_setting);
  if (slot === "sr_story_engine") return normalizeStoryEngine(k?.story_engine);
  if (slot === "sr_fulfillment_system") return normalizeFulfillmentSystem(k?.fulfillment_system);
  return normalizeConstraintRedlines(k?.constraint_redlines);
}

function pushSection(lines: string[], title: string, body: string) {
  const t = body.trim();
  if (!t) return;
  lines.push(`## ${title}`, t, "");
}

function pushListSection(lines: string[], title: string, items: string[]) {
  const list = items.map((x) => x.trim()).filter(Boolean);
  if (!list.length) return;
  lines.push(`## ${title}`);
  list.forEach((x, i) => lines.push(`${i + 1}. ${x}`));
  lines.push("");
}

export function formatSurfaceSettingExtracted(d: SurfaceSettingData): string {
  const lines: string[] = [];
  pushSection(lines, "一句话立意", d.premise);
  pushSection(lines, "核心冲突", d.core_conflict);
  pushSection(lines, "读者承诺", d.reader_promise);
  pushSection(lines, "目标读者", d.target_audience);
  pushSection(lines, "基调参照", d.tone_reference);
  pushSection(lines, "商业标签", d.commercial_tags);
  pushSection(lines, "扩展立意", d.extended_premise);
  return lines.join("\n").trim();
}

export function formatStoryEngineExtracted(d: StoryEngineData): string {
  const lines: string[] = [];
  pushSection(lines, "一句话立意", d.premise);
  if (d.bright_line.trim() || d.dark_line.trim()) {
    lines.push("## 明暗双线");
    if (d.bright_line.trim()) lines.push("### 明线", d.bright_line.trim(), "");
    if (d.dark_line.trim()) lines.push("### 暗线", d.dark_line.trim(), "");
  }
  pushSection(lines, "悬念设定", d.suspense_setup);
  pushSection(lines, "冲突引擎", d.conflict_engine);
  if (
    d.external_conflict.trim() ||
    d.internal_conflict.trim() ||
    d.relational_conflict.trim()
  ) {
    lines.push("## 冲突层级");
    if (d.external_conflict.trim()) lines.push("### 外部", d.external_conflict.trim(), "");
    if (d.internal_conflict.trim()) lines.push("### 内部", d.internal_conflict.trim(), "");
    if (d.relational_conflict.trim()) lines.push("### 关系", d.relational_conflict.trim(), "");
  }
  pushSection(lines, "推进循环", d.progression_cycle);
  pushSection(lines, "主角困局", d.protagonist_dilemma);
  return lines.join("\n").trim();
}

export function formatFulfillmentSystemExtracted(d: FulfillmentSystemData): string {
  const lines: string[] = [];
  pushSection(lines, "一句话立意", d.premise);
  pushSection(lines, "成长路径", d.growth_path);
  pushSection(lines, "结局质感", d.ending_texture);
  pushListSection(lines, "兑现语法", d.payoff_syntax);
  pushSection(lines, "情绪节奏", d.emotional_rhythm);
  pushListSection(lines, "张力原型", d.tension_circles);
  return lines.join("\n").trim();
}

export function formatConstraintRedlinesExtracted(d: ConstraintRedlinesData): string {
  const lines: string[] = [];
  pushSection(lines, "一句话立意", d.premise);
  pushListSection(lines, "约束红线", d.redlines);
  return lines.join("\n").trim();
}

export function formatStoryRulesBlockExtracted(slot: StoryRulesBlockSlot, data: StoryRulesBlockData): string {
  if (slot === "sr_surface_setting") return formatSurfaceSettingExtracted(data as SurfaceSettingData);
  if (slot === "sr_story_engine") return formatStoryEngineExtracted(data as StoryEngineData);
  if (slot === "sr_fulfillment_system") return formatFulfillmentSystemExtracted(data as FulfillmentSystemData);
  return formatConstraintRedlinesExtracted(data as ConstraintRedlinesData);
}

export function formatAllStoryRulesExtracted(
  blocks: Array<{ slot: StoryRulesBlockSlot; data: StoryRulesBlockData; title: string }>,
): string {
  const parts: string[] = [];
  for (const b of blocks) {
    const body = formatStoryRulesBlockExtracted(b.slot, b.data);
    if (!body) continue;
    parts.push(`# ${b.title}`, body);
  }
  return parts.join("\n\n").trim();
}

function fieldFilled(data: StoryRulesBlockData, def: StoryRulesFieldDef): boolean {
  const raw = (data as Record<string, unknown>)[def.key];
  if (def.kind === "list") {
    return Array.isArray(raw) && raw.some((x) => String(x).trim());
  }
  return String(raw ?? "").trim().length > 0;
}

export function listMissingStoryRulesFields(
  slot: StoryRulesBlockSlot,
  data: StoryRulesBlockData,
): string[] {
  return STORY_RULES_BLOCK_FIELDS[slot]
    .filter((def) => !fieldFilled(data, def))
    .map((def) => def.key);
}

export function parseStoryRulesBlockJson(
  slot: StoryRulesBlockSlot,
  text: string,
): StoryRulesBlockData | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const normalized = normalizeStoryRulesBlock(slot, o);
    const missing = listMissingStoryRulesFields(slot, normalized);
    return missing.length ? null : normalized;
  } catch {
    return null;
  }
}

export function buildStoryRulesBlockWholeInstruction(
  slot: StoryRulesBlockSlot,
  note: string,
  title: string,
): string {
  const fields = STORY_RULES_BLOCK_FIELDS[slot];
  const keys = fields.map((f) => f.key).join("、");
  const listKeys = fields.filter((f) => f.kind === "list").map((f) => f.key);
  const listHint = listKeys.length
    ? `列表字段 ${listKeys.join("、")} 须为非空字符串数组。`
    : "";
  return `${note.trim()}\n\n【卡片】${title}\n【须输出 JSON，键名】${keys}\n每项须有具体可写作内容，禁止空值与「待定」。${listHint}\n只输出 JSON 对象。`;
}
