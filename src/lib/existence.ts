/** 存在基础结构化（slot=wv_existence） */
export type ExistenceData = {
  premise: string;
  death: string;
  calendar: string;
  lifespan: string;
  disease_reproduction: string;
};

export function emptyExistence(): ExistenceData {
  return {
    premise: "",
    death: "",
    calendar: "",
    lifespan: "",
    disease_reproduction: "",
  };
}

export function normalizeExistence(
  raw: Partial<ExistenceData> | null | undefined,
): ExistenceData {
  const d = emptyExistence();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.death = raw.death ?? "";
  d.calendar = raw.calendar ?? "";
  d.lifespan = raw.lifespan ?? "";
  d.disease_reproduction = raw.disease_reproduction ?? "";
  return d;
}

export function formatExistenceExtracted(d: ExistenceData): string {
  const lines: string[] = [];
  const premise = d.premise.trim();
  if (premise) {
    lines.push("## 一句话立意", premise, "");
  }
  const death = d.death.trim();
  if (death) {
    lines.push("## 死亡", death, "");
  }
  const calendar = d.calendar.trim();
  if (calendar) {
    lines.push("## 历法", calendar, "");
  }
  const lifespan = d.lifespan.trim();
  if (lifespan) {
    lines.push("## 寿命", lifespan, "");
  }
  const dr = d.disease_reproduction.trim();
  if (dr) {
    lines.push("## 疫病与繁衍", dr, "");
  }
  return lines.join("\n").trim();
}
