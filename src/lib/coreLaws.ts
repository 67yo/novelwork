/** 核心法则：世界公理（名称 + 四元组）——可存树上公理卡 knowledge.world_axiom */
export type WorldAxiom = {
  name: string;
  statement: string;
  boundary: string;
  cost: string;
  mechanism: string;
};

/** 核心法则固定项（公理改挂子卡后 axioms 仅兼容旧数据 / 迁移） */
export type CoreLawsData = {
  premise: string;
  axioms: WorldAxiom[];
  taboos: string[];
  power_system: string;
  power_expression: string;
};

export const AXIOM_SLOT = "wv_axiom";

export function emptyAxiom(): WorldAxiom {
  return { name: "", statement: "", boundary: "", cost: "", mechanism: "" };
}

export function emptyCoreLaws(): CoreLawsData {
  return {
    premise: "",
    axioms: [],
    taboos: [""],
    power_system: "",
    power_expression: "",
  };
}

export function normalizeAxiom(raw: Partial<WorldAxiom> | null | undefined): WorldAxiom {
  return {
    name: raw?.name ?? "",
    statement: raw?.statement ?? "",
    boundary: raw?.boundary ?? "",
    cost: raw?.cost ?? "",
    mechanism: raw?.mechanism ?? "",
  };
}

/** 规范化：公理默认可空（由子卡承载） */
export function normalizeCoreLaws(raw: Partial<CoreLawsData> | null | undefined): CoreLawsData {
  const d = emptyCoreLaws();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.power_system = raw.power_system ?? "";
  d.power_expression = raw.power_expression ?? "";
  d.axioms = Array.isArray(raw.axioms)
    ? raw.axioms.map((a) => normalizeAxiom(a))
    : [];
  d.taboos =
    Array.isArray(raw.taboos) && raw.taboos.length
      ? raw.taboos.map((t) => (t == null ? "" : String(t)))
      : [""];
  return d;
}

export function formatAxiomBlock(a: WorldAxiom, fallbackIndex: number): string[] {
  const lines: string[] = [];
  const title = a.name.trim() || String(fallbackIndex);
  lines.push(`### ${title}`);
  if (a.statement.trim()) lines.push(`- 表述：${a.statement.trim()}`);
  if (a.boundary.trim()) lines.push(`- 边界：${a.boundary.trim()}`);
  if (a.cost.trim()) lines.push(`- 代价：${a.cost.trim()}`);
  if (a.mechanism.trim()) lines.push(`- 执行机制：${a.mechanism.trim()}`);
  lines.push("");
  return lines;
}

export function axiomHasContent(a: WorldAxiom): boolean {
  return !!(
    a.name.trim() ||
    a.statement.trim() ||
    a.boundary.trim() ||
    a.cost.trim() ||
    a.mechanism.trim()
  );
}

/** 写入 knowledge.extracted；axiomsOverride 优先（来自子卡） */
export function formatCoreLawsExtracted(
  d: CoreLawsData,
  axiomsOverride?: WorldAxiom[],
): string {
  const lines: string[] = [];
  const premise = d.premise.trim();
  if (premise) {
    lines.push("## 一句话立意", premise, "");
  }
  const axioms = (axiomsOverride ?? d.axioms).filter(axiomHasContent);
  if (axioms.length) {
    lines.push("## 世界公理");
    axioms.forEach((a, i) => lines.push(...formatAxiomBlock(a, i + 1)));
  }
  const taboos = d.taboos.map((t) => t.trim()).filter(Boolean);
  if (taboos.length) {
    lines.push("## 禁忌红线");
    taboos.forEach((t, i) => lines.push(`${i + 1}. ${t}`));
    lines.push("");
  }
  const ps = d.power_system.trim();
  if (ps) {
    lines.push("## 力量体系", ps, "");
  }
  const pe = d.power_expression.trim();
  if (pe) {
    lines.push("## 力量表现", pe, "");
  }
  return lines.join("\n").trim();
}

export function formatWorldAxiomExtracted(a: WorldAxiom): string {
  return formatAxiomBlock(a, 1).join("\n").trim();
}

/** 从模型输出里抠整条公理 JSON 对象 */
export function parseWorldAxiomJson(text: string): WorldAxiom | null {
  const s = text.trim();
  const start = s.indexOf("{");
  const end = s.lastIndexOf("}");
  if (start < 0 || end <= start) return null;
  try {
    const o = JSON.parse(s.slice(start, end + 1)) as Record<string, unknown>;
    if (!o || typeof o !== "object" || Array.isArray(o)) return null;
    const a = normalizeAxiom({
      name: String(o.name ?? o.名称 ?? "").trim(),
      statement: String(o.statement ?? o.表述 ?? "").trim(),
      boundary: String(o.boundary ?? o.边界 ?? "").trim(),
      cost: String(o.cost ?? o.代价 ?? "").trim(),
      mechanism: String(o.mechanism ?? o.执行机制 ?? o.机制 ?? "").trim(),
    });
    return axiomHasContent(a) ? a : null;
  } catch {
    return null;
  }
}

export function isAxiomSlot(slot: string | undefined | null): boolean {
  return (slot ?? "").trim() === AXIOM_SLOT;
}
