/** 引导预览：每张卡一个 `# 标题` 段，卡之间用 `---`，避免卷内 `# 三层结构` 被拆成新卡。 */

export type GuideMdSection = { title: string; body: string };

export function joinGuideMarkdown(sections: GuideMdSection[]): string {
  return sections
    .map((c) => `# ${c.title}\n\n${c.body}`.trim())
    .join("\n\n---\n\n");
}

function parseChunk(p: string): GuideMdSection {
  const s = p.trim();
  if (s.startsWith("# ")) {
    const i = s.indexOf("\n");
    if (i < 0) return { title: s.slice(2).trim(), body: "" };
    return { title: s.slice(2, i).trim(), body: s.slice(i + 1).trim() };
  }
  return { title: "", body: s };
}

export function splitGuideMarkdown(md: string, knownTitles: string[] = []): GuideMdSection[] {
  const trimmed = md.trim();
  if (!trimmed) return [];
  if (/^\s*---\s*$/m.test(trimmed)) {
    return trimmed
      .split(/^\s*---\s*$/m)
      .map((s) => s.trim())
      .filter(Boolean)
      .map(parseChunk);
  }
  const set = new Set(knownTitles);
  if (!set.size) return [parseChunk(trimmed)];
  const out: GuideMdSection[] = [];
  let title = "";
  let buf: string[] = [];
  const flush = () => {
    if (!title) return;
    out.push({ title, body: buf.join("\n").trim() });
  };
  for (const line of trimmed.split("\n")) {
    if (line.startsWith("# ")) {
      const t = line.slice(2).trim();
      if (set.has(t)) {
        flush();
        title = t;
        buf = [];
        continue;
      }
    }
    buf.push(line);
  }
  flush();
  return out.length ? out : [parseChunk(trimmed)];
}
