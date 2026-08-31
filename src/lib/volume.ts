/** 分卷结构化设定：本卷定位 / 三层结构 / 冲突层级 / 关键节点 */
export type VolumeLayerBlock = {
  chapters: string;
  description: string;
};

export type VolumeKeyBeat = {
  order: number;
  cost: string;
  description: string;
};

export type VolumeData = {
  positioning: string;
  layer_setup: VolumeLayerBlock;
  layer_confrontation: VolumeLayerBlock;
  layer_resolution: VolumeLayerBlock;
  conflict_external: string;
  conflict_internal: string;
  conflict_deep: string;
  key_beats: VolumeKeyBeat[];
};

export function emptyLayer(): VolumeLayerBlock {
  return { chapters: "", description: "" };
}

export function emptyKeyBeat(order = 1): VolumeKeyBeat {
  return { order, cost: "", description: "" };
}

export function emptyVolume(): VolumeData {
  return {
    positioning: "",
    layer_setup: emptyLayer(),
    layer_confrontation: emptyLayer(),
    layer_resolution: emptyLayer(),
    conflict_external: "",
    conflict_internal: "",
    conflict_deep: "",
    key_beats: [emptyKeyBeat(1)],
  };
}

export function normalizeLayer(raw: Partial<VolumeLayerBlock> | null | undefined): VolumeLayerBlock {
  return {
    chapters: raw?.chapters ?? "",
    description: raw?.description ?? "",
  };
}

export function normalizeKeyBeat(
  raw: Partial<VolumeKeyBeat> | null | undefined,
  fallbackOrder: number,
): VolumeKeyBeat {
  const order =
    typeof raw?.order === "number" && Number.isFinite(raw.order)
      ? Math.max(0, Math.floor(raw.order))
      : fallbackOrder;
  return {
    order,
    cost: raw?.cost ?? "",
    description: raw?.description ?? "",
  };
}

export function normalizeVolume(raw: Partial<VolumeData> | null | undefined): VolumeData {
  const d = emptyVolume();
  if (!raw) return d;
  d.positioning = raw.positioning ?? "";
  d.layer_setup = normalizeLayer(raw.layer_setup);
  d.layer_confrontation = normalizeLayer(raw.layer_confrontation);
  d.layer_resolution = normalizeLayer(raw.layer_resolution);
  d.conflict_external = raw.conflict_external ?? "";
  d.conflict_internal = raw.conflict_internal ?? "";
  d.conflict_deep = raw.conflict_deep ?? "";
  d.key_beats =
    Array.isArray(raw.key_beats) && raw.key_beats.length
      ? raw.key_beats.map((b, i) => normalizeKeyBeat(b, i + 1))
      : [emptyKeyBeat(1)];
  return d;
}

function pushSection(lines: string[], title: string, body: string) {
  const t = body.trim();
  if (!t) return;
  lines.push(`## ${title}`, t, "");
}

function pushLayer(lines: string[], title: string, layer: VolumeLayerBlock) {
  const ch = layer.chapters.trim();
  const desc = layer.description.trim();
  if (!ch && !desc) return;
  lines.push(`## ${title}`);
  if (ch) lines.push(`- 对应章节：${ch}`);
  if (desc) lines.push(desc);
  lines.push("");
}

export function formatVolumeExtracted(d: VolumeData): string {
  const lines: string[] = [];
  pushSection(lines, "本卷定位", d.positioning);
  const layers = [
    ["前置", d.layer_setup],
    ["对抗", d.layer_confrontation],
    ["收束", d.layer_resolution],
  ] as const;
  let anyLayer = false;
  for (const [, layer] of layers) {
    if (layer.chapters.trim() || layer.description.trim()) anyLayer = true;
  }
  if (anyLayer) {
    lines.push("# 三层结构", "");
    for (const [title, layer] of layers) pushLayer(lines, title, layer);
  }
  if (
    d.conflict_external.trim() ||
    d.conflict_internal.trim() ||
    d.conflict_deep.trim()
  ) {
    lines.push("# 冲突层级", "");
    pushSection(lines, "外部对抗", d.conflict_external);
    pushSection(lines, "内心矛盾", d.conflict_internal);
    pushSection(lines, "深层威胁", d.conflict_deep);
  }
  const beats = d.key_beats.filter(
    (b) => b.cost.trim() || b.description.trim() || b.order > 0,
  );
  if (beats.some((b) => b.cost.trim() || b.description.trim())) {
    lines.push("# 关键节点", "");
    for (const b of beats) {
      if (!b.cost.trim() && !b.description.trim()) continue;
      lines.push(`## ${b.order}`);
      if (b.cost.trim()) lines.push(`- 代价：${b.cost.trim()}`);
      if (b.description.trim()) lines.push(b.description.trim());
      lines.push("");
    }
  }
  return lines.join("\n").trim();
}

export function volumeHasContent(d: VolumeData): boolean {
  return formatVolumeExtracted(d).length > 0;
}

/** 从 formatVolumeExtracted 产出的 Markdown 反向解析；不匹配则整段落入 positioning */
export function parseVolumeFromFormatted(text: string): VolumeData {
  const raw = text.trim();
  if (!raw) return emptyVolume();

  type Section =
    | "none"
    | "positioning"
    | "layer_setup"
    | "layer_confrontation"
    | "layer_resolution"
    | "conflict_external"
    | "conflict_internal"
    | "conflict_deep"
    | { kind: "beat"; order: number };

  let section: Section = "none";
  let buf: string[] = [];
  let parsedAny = false;
  const d = emptyVolume();

  const flush = () => {
    const body = buf.join("\n").trim();
    buf = [];
    if (!body) return;
    parsedAny = true;
    if (section === "positioning") d.positioning = body;
    else if (section === "layer_setup") {
      if (body.startsWith("- 对应章节：")) d.layer_setup.chapters = body.slice("- 对应章节：".length).trim();
      else d.layer_setup.description = body;
    } else if (section === "layer_confrontation") {
      if (body.startsWith("- 对应章节：")) d.layer_confrontation.chapters = body.slice("- 对应章节：".length).trim();
      else d.layer_confrontation.description = body;
    } else if (section === "layer_resolution") {
      if (body.startsWith("- 对应章节：")) d.layer_resolution.chapters = body.slice("- 对应章节：".length).trim();
      else d.layer_resolution.description = body;
    } else if (section === "conflict_external") d.conflict_external = body;
    else if (section === "conflict_internal") d.conflict_internal = body;
    else if (section === "conflict_deep") d.conflict_deep = body;
    else if (typeof section === "object" && section.kind === "beat") {
      let cost = "";
      let desc = body;
      if (body.startsWith("- 代价：")) {
        const rest = body.slice("- 代价：".length);
        const idx = rest.indexOf("\n");
        cost = (idx >= 0 ? rest.slice(0, idx) : rest).trim();
        desc = idx >= 0 ? rest.slice(idx + 1).trim() : "";
      }
      d.key_beats.push({ order: section.order, cost, description: desc });
    }
  };

  const layerFromH2 = (title: string): Section | null => {
    if (title === "前置") return "layer_setup";
    if (title === "对抗") return "layer_confrontation";
    if (title === "收束") return "layer_resolution";
    return null;
  };
  const conflictFromH2 = (title: string): Section | null => {
    if (title === "外部对抗") return "conflict_external";
    if (title === "内心矛盾") return "conflict_internal";
    if (title === "深层威胁") return "conflict_deep";
    return null;
  };

  for (const line of raw.split("\n")) {
    if (line.startsWith("## ")) {
      flush();
      const title = line.slice(3).trim();
      if (title === "本卷定位") section = "positioning";
      else if (layerFromH2(title)) section = layerFromH2(title)!;
      else if (conflictFromH2(title)) section = conflictFromH2(title)!;
      else {
        const n = Number.parseInt(title, 10);
        section = Number.isFinite(n) ? { kind: "beat", order: n } : "none";
      }
      continue;
    }
    if (line.startsWith("# ")) {
      flush();
      section = "none";
      continue;
    }
    if (section === "none") continue;
    if (
      (section === "layer_setup" || section === "layer_confrontation" || section === "layer_resolution") &&
      line.startsWith("- 对应章节：")
    ) {
      flush();
      const ch = line.slice("- 对应章节：".length).trim();
      if (section === "layer_setup") d.layer_setup.chapters = ch;
      else if (section === "layer_confrontation") d.layer_confrontation.chapters = ch;
      else d.layer_resolution.chapters = ch;
      parsedAny = true;
      continue;
    }
    buf.push(line);
  }
  flush();

  if (!parsedAny) d.positioning = raw;
  return normalizeVolume(d);
}

/** 画布节点摘要：优先本卷定位 */
export function volumeDisplaySummary(
  volume: VolumeData | null | undefined,
): string {
  const v = normalizeVolume(volume);
  const pos = v.positioning.trim();
  if (pos) return pos;
  const excerpt = formatVolumeExtracted(v);
  return excerpt.length > 200 ? `${excerpt.slice(0, 200)}…` : excerpt;
}
