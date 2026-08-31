/** 人物卡完整结构化设定（写章注入用 formatCharacterExtracted） */

export type CharacterRelation = {
  name: string;
  relation: string;
  /** 相对当前角色，对对方的观感/定义 */
  definition: string;
  default_attitude: string;
  hidden_tension: string;
};

export type CharacterWorldPosition = {
  birth_class: string;
  faction: string;
  social_role: string;
  /** 例：赞同｜反对 + 说明 */
  conflict_stance: string;
};

export type CharacterWorldAnchors = {
  /** 核心法则公理名/标题 */
  embodies_law: string;
  embodies_note: string;
  shaped_by_law: string;
  shaped_by_note: string;
  will_challenge: string;
};

/** 作者视角：终将证立｜终将证伪｜悬而未决 */
export type AuthorVerdict = "" | "prove" | "disprove" | "unresolved";

export type CharacterCoreBelief = {
  belief: string;
  author_verdict: AuthorVerdict;
  source: string;
};

export type CharacterDeep = {
  desire_surface: string;
  desire_deep: string;
  fear: string;
  fear_source: string;
  secret_content: string;
  secret_who_knows: string;
  /** 该知但未知道：人名 */
  secret_should_know: string;
  secret_exposure: string;
  line_trigger: string;
  line_source: string;
  line_reaction: string;
  trauma_wound: string;
  trauma_trigger: string;
  trauma_stress: string;
  trauma_imprint: string;
  contradiction_poles: string;
  contradiction_source: string;
  contradiction_trajectory: string;
  arc_growth: string;
  arc_fall: string;
  arc_choice: string;
};

export type CharacterVoice = {
  positioning: string;
  cognitive_filter: string;
  /** 习惯动作、姿态、微表情 */
  body_language: string;
  sentence_length: string;
  pause: string;
  patterns: string;
  catchphrases: string[];
  emotion_anger: string;
  emotion_tense: string;
  emotion_mask: string;
  emotion_sad: string;
  emotion_happy: string;
  banned: string;
};

/**
 * 人物卡载荷。保留旧扁平字段以便旧树/MCP/画布摘要兼容；
 * 保存时用 syncLegacyFields 从结构化同步。
 */
export type CharacterSheet = {
  prompt: string;
  image_path: string;
};

export type CharacterCard = {
  role: string;
  personality: string;
  motto: string;
  gender: string;
  style: string;
  alignment: string;
  age: string;
  constraints: string;
  /** 别称 */
  aliases: string;
  world_position: CharacterWorldPosition;
  world_anchors: CharacterWorldAnchors;
  relations: CharacterRelation[];
  core_belief: CharacterCoreBelief;
  deep: CharacterDeep;
  voice: CharacterVoice;
  sheet: CharacterSheet;
};

export function emptySheet(): CharacterSheet {
  return { prompt: "", image_path: "" };
}

export function buildCharacterSheetPrompt(name: string, card: CharacterCard): string {
  const bits = [name, card.gender, card.age, card.role, card.style, card.voice.body_language]
    .map((s) => s.trim())
    .filter(Boolean);
  return `character design turnaround sheet of one person, four full-body views left to right: front view, left profile, back view, right profile, same face outfit and proportions, full body head to toe, even spacing, plain light gray studio background, ${bits.join(", ")}, clean illustration, no text labels, no extra characters`;
}

export function emptyWorldPosition(): CharacterWorldPosition {
  return {
    birth_class: "",
    faction: "",
    social_role: "",
    conflict_stance: "",
  };
}

export function emptyWorldAnchors(): CharacterWorldAnchors {
  return {
    embodies_law: "",
    embodies_note: "",
    shaped_by_law: "",
    shaped_by_note: "",
    will_challenge: "",
  };
}

export function emptyRelation(): CharacterRelation {
  return {
    name: "",
    relation: "",
    definition: "",
    default_attitude: "",
    hidden_tension: "",
  };
}

export function emptyCoreBelief(): CharacterCoreBelief {
  return { belief: "", author_verdict: "", source: "" };
}

export function emptyDeep(): CharacterDeep {
  return {
    desire_surface: "",
    desire_deep: "",
    fear: "",
    fear_source: "",
    secret_content: "",
    secret_who_knows: "",
    secret_should_know: "",
    secret_exposure: "",
    line_trigger: "",
    line_source: "",
    line_reaction: "",
    trauma_wound: "",
    trauma_trigger: "",
    trauma_stress: "",
    trauma_imprint: "",
    contradiction_poles: "",
    contradiction_source: "",
    contradiction_trajectory: "",
    arc_growth: "",
    arc_fall: "",
    arc_choice: "",
  };
}

export function emptyVoice(): CharacterVoice {
  return {
    positioning: "",
    cognitive_filter: "",
    body_language: "",
    sentence_length: "",
    pause: "",
    patterns: "",
    catchphrases: [""],
    emotion_anger: "",
    emotion_tense: "",
    emotion_mask: "",
    emotion_sad: "",
    emotion_happy: "",
    banned: "",
  };
}

export function emptyCharacterCard(): CharacterCard {
  return {
    role: "",
    personality: "",
    motto: "",
    gender: "",
    style: "",
    alignment: "",
    age: "",
    constraints: "",
    aliases: "",
    world_position: emptyWorldPosition(),
    world_anchors: emptyWorldAnchors(),
    relations: [],
    core_belief: emptyCoreBelief(),
    deep: emptyDeep(),
    voice: emptyVoice(),
    sheet: emptySheet(),
  };
}

function str(v: unknown): string {
  return v == null ? "" : String(v);
}

function asObj(v: unknown): Record<string, unknown> | null {
  return v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : null;
}

function normalizeVerdict(v: unknown): AuthorVerdict {
  const s = str(v).trim();
  if (s === "prove" || s === "终将证立" || s === "证立") return "prove";
  if (s === "disprove" || s === "终将证伪" || s === "证伪") return "disprove";
  if (s === "unresolved" || s === "悬而未决" || s === "未决") return "unresolved";
  return "";
}

export function normalizeRelation(
  raw: Partial<CharacterRelation> | null | undefined,
): CharacterRelation {
  return {
    name: str(raw?.name),
    relation: str(raw?.relation),
    definition: str(raw?.definition),
    default_attitude: str(raw?.default_attitude),
    hidden_tension: str(raw?.hidden_tension),
  };
}

export function normalizeCharacterCard(
  raw: Partial<CharacterCard> | null | undefined,
): CharacterCard {
  const d = emptyCharacterCard();
  if (!raw) return d;

  d.role = str(raw.role);
  d.personality = str(raw.personality);
  d.motto = str(raw.motto);
  d.gender = str(raw.gender);
  d.style = str(raw.style);
  d.alignment = str(raw.alignment);
  d.age = str(raw.age);
  d.constraints = str(raw.constraints);
  d.aliases = str(raw.aliases);

  const wp = asObj(raw.world_position) ?? {};
  d.world_position = {
    birth_class: str(wp.birth_class ?? wp.出生阶层),
    faction: str(wp.faction ?? wp.阵营归属 ?? raw.alignment),
    social_role: str(wp.social_role ?? wp.社会角色 ?? raw.role),
    conflict_stance: str(wp.conflict_stance ?? wp.核心冲突立场),
  };
  // 旧卡无 world_position 时回填
  if (!asObj(raw.world_position)) {
    if (!d.world_position.faction && d.alignment) d.world_position.faction = d.alignment;
    if (!d.world_position.social_role && d.role) d.world_position.social_role = d.role;
  }

  const wa = asObj(raw.world_anchors) ?? {};
  d.world_anchors = {
    embodies_law: str(wa.embodies_law ?? wa.体现法则),
    embodies_note: str(wa.embodies_note ?? wa.体现法则注),
    shaped_by_law: str(wa.shaped_by_law ?? wa.受塑于),
    shaped_by_note: str(wa.shaped_by_note ?? wa.受塑于注),
    will_challenge: str(wa.will_challenge ?? wa.或将挑战),
  };

  d.relations = Array.isArray(raw.relations)
    ? raw.relations.map((r) => normalizeRelation(r))
    : [];

  const cb = asObj(raw.core_belief) ?? {};
  d.core_belief = {
    belief: str(cb.belief ?? cb.核心信念 ?? raw.motto),
    author_verdict: normalizeVerdict(cb.author_verdict ?? cb.作者视角),
    source: str(cb.source ?? cb.信念来源),
  };
  if (!asObj(raw.core_belief) && d.motto && !d.core_belief.belief) {
    d.core_belief.belief = d.motto;
  }

  const dp = asObj(raw.deep) ?? {};
  const desire = asObj(dp.desire) ?? {};
  const fear = asObj(dp.fear) ?? {};
  const secret = asObj(dp.secret) ?? {};
  const line = asObj(dp.bottom_line ?? dp.line) ?? {};
  const trauma = asObj(dp.trauma) ?? {};
  const contra = asObj(dp.contradiction) ?? {};
  const arc = asObj(dp.arc) ?? {};
  d.deep = {
    desire_surface: str(dp.desire_surface ?? desire.surface ?? desire.表层目标),
    desire_deep: str(dp.desire_deep ?? desire.deep ?? desire.深层渴望),
    fear: str(dp.fear ?? fear.fear ?? fear.恐惧),
    fear_source: str(dp.fear_source ?? fear.source ?? fear.来源),
    secret_content: str(dp.secret_content ?? secret.content ?? secret.内容),
    secret_who_knows: str(dp.secret_who_knows ?? secret.who_knows ?? secret.谁知道),
    secret_should_know: str(
      dp.secret_should_know ?? secret.should_know ?? secret.该知但未知道,
    ),
    secret_exposure: str(dp.secret_exposure ?? secret.exposure ?? secret.暴露后果),
    line_trigger: str(dp.line_trigger ?? line.trigger ?? line.触线条件),
    line_source: str(dp.line_source ?? line.source ?? line.来源),
    line_reaction: str(dp.line_reaction ?? line.reaction ?? line.越线反应),
    trauma_wound: str(dp.trauma_wound ?? trauma.wound ?? trauma.伤口),
    trauma_trigger: str(dp.trauma_trigger ?? trauma.trigger ?? trauma.触发器),
    trauma_stress: str(dp.trauma_stress ?? trauma.stress ?? trauma.应激反应),
    trauma_imprint: str(dp.trauma_imprint ?? trauma.imprint ?? trauma.行为烙印),
    contradiction_poles: str(
      dp.contradiction_poles ?? contra.poles ?? contra.冲突两级,
    ),
    contradiction_source: str(dp.contradiction_source ?? contra.source ?? contra.来源),
    contradiction_trajectory: str(
      dp.contradiction_trajectory ?? contra.trajectory ?? contra.可能走向,
    ),
    arc_growth: str(dp.arc_growth ?? arc.growth ?? arc.成长向),
    arc_fall: str(dp.arc_fall ?? arc.fall ?? arc.堕落向),
    arc_choice: str(dp.arc_choice ?? arc.choice ?? arc.关键抉择),
  };
  if (!asObj(raw.deep) && d.personality) {
    d.deep.desire_surface = d.deep.desire_surface || d.personality;
  }

  const vo = asObj(raw.voice) ?? {};
  const syntax = asObj(vo.syntax) ?? {};
  const emotion = asObj(vo.emotion) ?? {};
  const phrasesRaw = vo.catchphrases ?? syntax.catchphrases ?? syntax.口头禅;
  const phrases = Array.isArray(phrasesRaw)
    ? phrasesRaw.map((x) => str(x))
    : typeof phrasesRaw === "string" && phrasesRaw.trim()
      ? [str(phrasesRaw)]
      : [""];
  d.voice = {
    positioning: str(vo.positioning ?? vo.声音定位 ?? raw.style),
    cognitive_filter: str(vo.cognitive_filter ?? vo.认知滤镜),
    body_language: str(vo.body_language ?? vo.肢体语言),
    sentence_length: str(vo.sentence_length ?? syntax.sentence_length ?? syntax.长短句偏好),
    pause: str(vo.pause ?? syntax.pause ?? syntax.停顿习惯),
    patterns: str(vo.patterns ?? syntax.patterns ?? syntax.常用句式),
    catchphrases: phrases.length ? phrases : [""],
    emotion_anger: str(vo.emotion_anger ?? emotion.anger ?? emotion.愤怒时),
    emotion_tense: str(vo.emotion_tense ?? emotion.tense ?? emotion.紧张时),
    emotion_mask: str(vo.emotion_mask ?? emotion.mask ?? emotion.掩饰时),
    emotion_sad: str(vo.emotion_sad ?? emotion.sad ?? emotion.伤心时),
    emotion_happy: str(vo.emotion_happy ?? emotion.happy ?? emotion.开心时),
    banned: str(vo.banned ?? vo.禁用表达 ?? vo.不会说的话),
  };
  if (!asObj(raw.voice) && d.style) {
    d.voice.positioning = d.voice.positioning || d.style;
  }

  const sh = asObj(raw.sheet) ?? {};
  d.sheet = {
    prompt: str(sh.prompt),
    image_path: str(sh.image_path),
  };

  return d;
}

/** 把结构化字段回写到旧扁平字段，供画布摘要与旧注入路径使用 */
export function syncLegacyFields(card: CharacterCard): CharacterCard {
  const c = normalizeCharacterCard(card);
  c.role = c.world_position.social_role.trim() || c.role;
  c.alignment = c.world_position.faction.trim() || c.alignment;
  c.motto = c.core_belief.belief.trim() || c.motto;
  c.style = c.voice.positioning.trim() || c.style;
  const personalityBits = [
    c.deep.desire_surface,
    c.deep.desire_deep,
    c.deep.fear,
    c.core_belief.belief,
  ]
    .map((s) => s.trim())
    .filter(Boolean);
  if (personalityBits.length) c.personality = personalityBits.join("；");
  const extracted = formatCharacterExtracted(c);
  if (extracted) c.constraints = extracted;
  return c;
}

function push(lines: string[], title: string, body: string) {
  const b = body.trim();
  if (!b) return;
  lines.push(`### ${title}`, b, "");
}

function pushLabeled(
  lines: string[],
  title: string,
  items: { label: string; value: string }[],
) {
  const bits = items
    .map((it) => ({ label: it.label, value: it.value.trim() }))
    .filter((it) => it.value);
  if (!bits.length) return;
  lines.push(`### ${title}`);
  for (const it of bits) lines.push(`- ${it.label}：${it.value}`);
  lines.push("");
}

export function authorVerdictLabel(v: AuthorVerdict): string {
  if (v === "prove") return "终将证立";
  if (v === "disprove") return "终将证伪";
  if (v === "unresolved") return "悬而未决";
  return "";
}

/** 写作注入用完整人设正文 */
export function formatCharacterExtracted(card: CharacterCard, trueName = ""): string {
  const c = normalizeCharacterCard(card);
  const lines: string[] = [];
  const name = trueName.trim();
  if (name) lines.push(`## 真名`, name, "");
  if (c.aliases.trim()) lines.push(`## 别称`, c.aliases.trim(), "");
  if (c.gender.trim()) lines.push(`## 性别`, c.gender.trim(), "");
  if (c.age.trim()) lines.push(`## 年龄`, c.age.trim(), "");

  const wp = c.world_position;
  if (
    wp.birth_class.trim() ||
    wp.faction.trim() ||
    wp.social_role.trim() ||
    wp.conflict_stance.trim()
  ) {
    lines.push(`## 世界位置`);
    if (wp.birth_class.trim()) lines.push(`- 出生阶层：${wp.birth_class.trim()}`);
    if (wp.faction.trim()) lines.push(`- 阵营归属：${wp.faction.trim()}`);
    if (wp.social_role.trim()) lines.push(`- 社会角色：${wp.social_role.trim()}`);
    if (wp.conflict_stance.trim()) lines.push(`- 核心冲突立场：${wp.conflict_stance.trim()}`);
    lines.push("");
  }

  const wa = c.world_anchors;
  if (
    wa.embodies_law.trim() ||
    wa.embodies_note.trim() ||
    wa.shaped_by_law.trim() ||
    wa.shaped_by_note.trim() ||
    wa.will_challenge.trim()
  ) {
    lines.push(`## 世界锚点`);
    if (wa.embodies_law.trim() || wa.embodies_note.trim()) {
      lines.push(
        `- 体现法则：${[wa.embodies_law.trim(), wa.embodies_note.trim()].filter(Boolean).join(" — ")}`,
      );
    }
    if (wa.shaped_by_law.trim() || wa.shaped_by_note.trim()) {
      lines.push(
        `- 受塑于：${[wa.shaped_by_law.trim(), wa.shaped_by_note.trim()].filter(Boolean).join(" — ")}`,
      );
    }
    if (wa.will_challenge.trim()) lines.push(`- 或将挑战：${wa.will_challenge.trim()}`);
    lines.push("");
  }

  const rels = c.relations.filter(
    (r) =>
      r.name.trim() ||
      r.relation.trim() ||
      r.definition.trim() ||
      r.default_attitude.trim() ||
      r.hidden_tension.trim(),
  );
  if (rels.length) {
    lines.push(`## 关系网络`);
    rels.forEach((r, i) => {
      lines.push(`### ${r.name.trim() || i + 1}`);
      if (r.relation.trim()) lines.push(`- 关系：${r.relation.trim()}`);
      if (r.definition.trim()) lines.push(`- 对角色的定义：${r.definition.trim()}`);
      if (r.default_attitude.trim()) lines.push(`- 默认态度：${r.default_attitude.trim()}`);
      if (r.hidden_tension.trim()) lines.push(`- 隐藏张力：${r.hidden_tension.trim()}`);
      lines.push("");
    });
  }

  const cb = c.core_belief;
  if (cb.belief.trim() || cb.author_verdict || cb.source.trim()) {
    lines.push(`## 核心信念`);
    if (cb.belief.trim()) lines.push(`- 核心信念：${cb.belief.trim()}`);
    const verd = authorVerdictLabel(cb.author_verdict);
    if (verd) lines.push(`- 作者视角：${verd}`);
    if (cb.source.trim()) lines.push(`- 信念来源：${cb.source.trim()}`);
    lines.push("");
  }

  const dp = c.deep;
  const deepStart = lines.length;
  lines.push(`## 深层维度`);
  pushLabeled(lines, "核心欲望", [
    { label: "表层目标", value: dp.desire_surface },
    { label: "深层渴望", value: dp.desire_deep },
  ]);
  pushLabeled(lines, "深层恐惧", [
    { label: "恐惧", value: dp.fear },
    { label: "来源", value: dp.fear_source },
  ]);
  pushLabeled(lines, "秘密", [
    { label: "内容", value: dp.secret_content },
    { label: "谁知道", value: dp.secret_who_knows },
    { label: "该知但未知道", value: dp.secret_should_know },
    { label: "暴露后果", value: dp.secret_exposure },
  ]);
  pushLabeled(lines, "底线", [
    { label: "触线条件", value: dp.line_trigger },
    { label: "来源", value: dp.line_source },
    { label: "越线反应", value: dp.line_reaction },
  ]);
  pushLabeled(lines, "关键创伤", [
    { label: "伤口", value: dp.trauma_wound },
    { label: "触发器", value: dp.trauma_trigger },
    { label: "应激反应", value: dp.trauma_stress },
    { label: "行为烙印", value: dp.trauma_imprint },
  ]);
  pushLabeled(lines, "内在矛盾", [
    { label: "冲突两级", value: dp.contradiction_poles },
    { label: "来源", value: dp.contradiction_source },
    { label: "可能走向", value: dp.contradiction_trajectory },
  ]);
  pushLabeled(lines, "人物弧光", [
    { label: "成长向", value: dp.arc_growth },
    { label: "堕落向", value: dp.arc_fall },
    { label: "关键抉择", value: dp.arc_choice },
  ]);
  if (lines.length === deepStart + 1) lines.pop();

  const vo = c.voice;
  const voiceBits =
    vo.positioning.trim() ||
    vo.cognitive_filter.trim() ||
    vo.body_language.trim() ||
    vo.sentence_length.trim() ||
    vo.pause.trim() ||
    vo.patterns.trim() ||
    vo.catchphrases.some((p) => p.trim()) ||
    vo.emotion_anger.trim() ||
    vo.emotion_tense.trim() ||
    vo.emotion_mask.trim() ||
    vo.emotion_sad.trim() ||
    vo.emotion_happy.trim() ||
    vo.banned.trim();
  if (voiceBits) {
    lines.push(`## 角色声线`);
    push(lines, "声音定位", vo.positioning);
    push(lines, "认知滤镜", vo.cognitive_filter);
    push(lines, "肢体语言", vo.body_language);
    {
      const bits = [
        vo.sentence_length && `长短句偏好：${vo.sentence_length}`,
        vo.pause && `停顿习惯：${vo.pause}`,
        vo.patterns && `常用句式：${vo.patterns}`,
      ].filter(Boolean) as string[];
      const phrases = vo.catchphrases.map((p) => p.trim()).filter(Boolean);
      if (bits.length || phrases.length) {
        lines.push(`### 句法指纹`);
        for (const b of bits) lines.push(`- ${b}`);
        for (const p of phrases) lines.push(`- 口头禅：${p}`);
        lines.push("");
      }
    }
    {
      const bits = [
        vo.emotion_anger && `愤怒时：${vo.emotion_anger}`,
        vo.emotion_tense && `紧张时：${vo.emotion_tense}`,
        vo.emotion_mask && `掩饰时：${vo.emotion_mask}`,
        vo.emotion_sad && `伤心时：${vo.emotion_sad}`,
        vo.emotion_happy && `开心时：${vo.emotion_happy}`,
      ].filter(Boolean) as string[];
      if (bits.length) lines.push(`### 情绪变化`, ...bits.map((b) => `- ${b}`), "");
    }
    push(lines, "禁用表达（不会说的话）", vo.banned);
  }

  return lines.join("\n").trim();
}

function pick(o: Record<string, unknown>, ...keys: string[]): unknown {
  for (const k of keys) {
    if (o[k] != null && o[k] !== "") return o[k];
  }
  return undefined;
}

/** AI 整卡改写：从 JSON 解析（兼容中文键） */
export function parseCharacterCardJson(text: string): CharacterCard | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const card = normalizeCharacterCard({
      gender: str(pick(o, "gender", "性别")),
      age: str(pick(o, "age", "年龄")),
      aliases: str(pick(o, "aliases", "别称")),
      role: str(pick(o, "role", "社会角色", "身份")),
      alignment: str(pick(o, "alignment", "阵营归属", "阵营")),
      personality: str(pick(o, "personality", "性格")),
      style: str(pick(o, "style", "行事风格")),
      motto: str(pick(o, "motto", "座右铭")),
      constraints: str(pick(o, "constraints", "约束")),
      world_position: (pick(o, "world_position", "世界位置") as CharacterWorldPosition) ?? {
        birth_class: str(pick(o, "birth_class", "出生阶层")),
        faction: str(pick(o, "faction", "阵营归属")),
        social_role: str(pick(o, "social_role", "社会角色")),
        conflict_stance: str(pick(o, "conflict_stance", "核心冲突立场")),
      },
      world_anchors: (pick(o, "world_anchors", "世界锚点") as CharacterWorldAnchors) ?? undefined,
      relations: (pick(o, "relations", "关系网络") as CharacterRelation[]) ?? undefined,
      core_belief: (pick(o, "core_belief", "核心信念") as CharacterCoreBelief) ?? undefined,
      deep: (pick(o, "deep", "深层维度") as CharacterDeep) ?? undefined,
      voice: (pick(o, "voice", "角色声线") as CharacterVoice) ?? undefined,
    } as Partial<CharacterCard>);
    // 若几乎全空则失败
    const md = formatCharacterExtracted(card);
    return md.length > 0 || card.gender || card.age || card.aliases ? card : null;
  } catch {
    return null;
  }
}

/** 传给 rewrite_text_field 的字段名；后端据此切换人物整卡 JSON 提示 */
export const CHARACTER_WHOLE_AI_FIELD_LABEL = "__character_card_whole__";

function trimReq(s: string): boolean {
  return s.trim().length > 0;
}

/** 整卡 AI 改写后须非空的字段 id（用于校验与错误提示） */
export function listMissingCharacterFields(
  card: CharacterCard,
  trueName = "",
): string[] {
  const c = normalizeCharacterCard(card);
  const miss: string[] = [];
  if (!trimReq(trueName)) miss.push("trueName");
  if (!trimReq(c.gender)) miss.push("gender");
  if (!trimReq(c.age)) miss.push("age");
  if (!trimReq(c.aliases)) miss.push("aliases");

  const wp = c.world_position;
  if (!trimReq(wp.birth_class)) miss.push("birthClass");
  if (!trimReq(wp.faction)) miss.push("faction");
  if (!trimReq(wp.social_role)) miss.push("socialRole");
  if (!trimReq(wp.conflict_stance)) miss.push("conflictStance");

  const wa = c.world_anchors;
  if (!trimReq(wa.embodies_law)) miss.push("embodiesLaw");
  if (!trimReq(wa.embodies_note)) miss.push("embodiesNote");
  if (!trimReq(wa.shaped_by_law)) miss.push("shapedBy");
  if (!trimReq(wa.shaped_by_note)) miss.push("shapedByNote");
  if (!trimReq(wa.will_challenge)) miss.push("willChallenge");

  if (!c.relations.length) {
    miss.push("relations");
  } else {
    for (let i = 0; i < c.relations.length; i++) {
      const r = c.relations[i]!;
      const prefix = `rel${i + 1}`;
      if (!trimReq(r.name)) miss.push(`${prefix}Name`);
      if (!trimReq(r.relation)) miss.push(`${prefix}Relation`);
      if (!trimReq(r.definition)) miss.push(`${prefix}Definition`);
      if (!trimReq(r.default_attitude)) miss.push(`${prefix}Attitude`);
      if (!trimReq(r.hidden_tension)) miss.push(`${prefix}Tension`);
    }
  }

  const cb = c.core_belief;
  if (!trimReq(cb.belief)) miss.push("belief");
  if (!cb.author_verdict) miss.push("authorVerdict");
  if (!trimReq(cb.source)) miss.push("beliefSource");

  const dp = c.deep;
  const deepMap: [string, string][] = [
    ["desireSurface", dp.desire_surface],
    ["desireDeep", dp.desire_deep],
    ["fearWhat", dp.fear],
    ["fearSource", dp.fear_source],
    ["secretContent", dp.secret_content],
    ["secretWho", dp.secret_who_knows],
    ["secretShouldKnow", dp.secret_should_know],
    ["secretExposure", dp.secret_exposure],
    ["lineTrigger", dp.line_trigger],
    ["lineSource", dp.line_source],
    ["lineReaction", dp.line_reaction],
    ["traumaWound", dp.trauma_wound],
    ["traumaTrigger", dp.trauma_trigger],
    ["traumaStress", dp.trauma_stress],
    ["traumaImprint", dp.trauma_imprint],
    ["contraPoles", dp.contradiction_poles],
    ["contraSource", dp.contradiction_source],
    ["contraTrajectory", dp.contradiction_trajectory],
    ["arcGrowth", dp.arc_growth],
    ["arcFall", dp.arc_fall],
    ["arcChoice", dp.arc_choice],
  ];
  for (const [id, val] of deepMap) {
    if (!trimReq(val)) miss.push(id);
  }

  const vo = c.voice;
  if (!trimReq(vo.positioning)) miss.push("voicePos");
  if (!trimReq(vo.cognitive_filter)) miss.push("cognitiveFilter");
  if (!trimReq(vo.body_language)) miss.push("bodyLanguage");
  if (!trimReq(vo.sentence_length)) miss.push("sentenceLength");
  if (!trimReq(vo.pause)) miss.push("pause");
  if (!trimReq(vo.patterns)) miss.push("patterns");
  if (!vo.catchphrases.some((p) => trimReq(p))) miss.push("catchphrases");
  if (!trimReq(vo.emotion_anger)) miss.push("emAnger");
  if (!trimReq(vo.emotion_tense)) miss.push("emTense");
  if (!trimReq(vo.emotion_mask)) miss.push("emMask");
  if (!trimReq(vo.emotion_sad)) miss.push("emSad");
  if (!trimReq(vo.emotion_happy)) miss.push("emHappy");
  if (!trimReq(vo.banned)) miss.push("banned");

  return miss;
}

export function isCharacterCardCompleteForAi(card: CharacterCard, trueName = ""): boolean {
  return listMissingCharacterFields(card, trueName).length === 0;
}

export type CharacterWholeAiOptions = {
  trueName?: string;
  lawOptions?: string[];
  characterNames?: string[];
};

/** 人物卡整卡 AI 改写：完整 JSON 模式与必填字段说明 */
export function buildCharacterWholeAiInstruction(
  userNote: string,
  opts: CharacterWholeAiOptions = {},
): string {
  const laws = (opts.lawOptions ?? []).filter((s) => s.trim());
  const names = (opts.characterNames ?? []).filter((s) => s.trim());
  const nameHint = opts.trueName?.trim()
    ? `当前真名：${opts.trueName.trim()}（须在 JSON 的 true_name/真名 中保留或按提示词调整）。`
    : "true_name/真名 须非空。";
  const lawHint = laws.length
    ? `世界锚点的 embodies_law/体现法则、shaped_by_law/受塑于 优先从下列核心法则/公理选取并写辅助说明：${laws.join("、")}。`
    : "";
  const relHint = names.length
    ? `关系网络中的人名可优先使用本书已有人物：${names.join("、")}。`
    : "";

  return `${userNote.trim()}

【硬性要求】
1. 只输出一个 JSON 对象；不要解释、不要 markdown 代码围栏。
2. 下列每一个字段都必须有具体、可写作的内容；禁止留空、禁止省略键；禁止写「待定」「暂无」「未知」「略」。
3. ${nameHint}
4. relations/关系网络：至少 1 条；每条须含 name、relation、definition、default_attitude、hidden_tension（均非空）。
5. core_belief.author_verdict 须为 prove | disprove | unresolved（或 终将证立 | 终将证伪 | 悬而未决）。
6. voice.catchphrases 为至少 1 个非空字符串的数组（建议 2 条口头禅）。
7. ${lawHint}
8. ${relHint}

【JSON 结构（键名可用英文或中文，须填满）】
{
  "true_name": "李四",
  "gender": "男",
  "age": "外表中年，实际800岁",
  "aliases": "别称",
  "world_position": {
    "birth_class": "出生阶层",
    "faction": "阵营归属",
    "social_role": "社会角色",
    "conflict_stance": "赞同｜反对 + 说明"
  },
  "world_anchors": {
    "embodies_law": "公理/法则名",
    "embodies_note": "体现说明",
    "shaped_by_law": "公理/法则名",
    "shaped_by_note": "受塑说明",
    "will_challenge": "或将挑战"
  },
  "relations": [
    {
      "name": "张三",
      "relation": "关系",
      "definition": "对对方的观感",
      "default_attitude": "默认态度",
      "hidden_tension": "隐藏张力"
    }
  ],
  "core_belief": {
    "belief": "核心信念",
    "author_verdict": "prove|disprove|unresolved",
    "source": "信念来源"
  },
  "deep": {
    "desire_surface": "表层目标",
    "desire_deep": "深层渴望",
    "fear": "恐惧",
    "fear_source": "恐惧来源",
    "secret_content": "秘密内容",
    "secret_who_knows": "谁知道",
    "secret_should_know": "该知但未知道（人名）",
    "secret_exposure": "暴露后果",
    "line_trigger": "触线条件",
    "line_source": "底线来源",
    "line_reaction": "越线反应",
    "trauma_wound": "伤口",
    "trauma_trigger": "触发器",
    "trauma_stress": "应激反应",
    "trauma_imprint": "行为烙印",
    "contradiction_poles": "冲突两级",
    "contradiction_source": "矛盾来源",
    "contradiction_trajectory": "可能走向",
    "arc_growth": "成长向",
    "arc_fall": "堕落向",
    "arc_choice": "关键抉择"
  },
  "voice": {
    "positioning": "声音定位",
    "cognitive_filter": "认知滤镜",
    "body_language": "肢体语言",
    "sentence_length": "长短句偏好",
    "pause": "停顿习惯",
    "patterns": "常用句式",
    "catchphrases": ["口头禅1", "口头禅2"],
    "emotion_anger": "愤怒时",
    "emotion_tense": "紧张时",
    "emotion_mask": "掩饰时",
    "emotion_sad": "伤心时",
    "emotion_happy": "开心时",
    "banned": "不会说的话"
  }
}`;
}
