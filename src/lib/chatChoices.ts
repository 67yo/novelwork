/** Parse clickable choices from an assistant chat reply. */

export type ChatChoices =
  | { kind: "yn"; multi: false }
  | { kind: "list"; multi: boolean; options: string[] };

const OPT_RE =
  /^(?:[-*•]|[0-9]{1,2}[.)、]|[A-Za-z][.)])\s+(?:\[([ xX])\]\s*)?(.+)$/;

const YN_RE =
  /是否|要不要|需不需要|需要吗|需要麼|需要么|可以吗|可以嗎|好吗|好嗎|要吗|要嗎|是不是|需要[^。\n]{0,48}[吗麼么]|yes\s*\/\s*no|\by\/n\b|yes or no/i;

/** 进度确认类是否题：不当成可点选项（仍靠 system 禁止模型乱问）。 */
const FLUFF_YN_RE =
  /要不要继续|是否继续|继续[吗麼么]|接着[吗麼么]|下一步|要不要开始|是否开始(?!写)|开始执行|继续下一项|继续未完成|可以继续吗|want me to continue|shall i continue/i;

const MULTI_RE = /可多选|可以多选|多选|select all|multi[- ]?select|multiple choice/i;

const LIST_CUE_RE =
  /请选择|請選擇|选一个|選一個|选哪|選哪|要哪|可多选|可以多选|哪一项|哪一項|还是|還是|choose|pick one|select one/i;

function stripMd(s: string): string {
  return s.replace(/\*\*(.+?)\*\*/g, "$1").replace(/`([^`]+)`/g, "$1").trim();
}

function visibleLines(text: string): string[] {
  const out: string[] = [];
  let fence = false;
  for (const raw of text.split(/\r?\n/)) {
    if (raw.trim().startsWith("```")) {
      fence = !fence;
      continue;
    }
    if (!fence) out.push(raw);
  }
  return out;
}

function parseOptionLine(line: string): { label: string; box: boolean } | null {
  const m = line.trim().match(OPT_RE);
  if (!m) return null;
  const label = stripMd(m[2] ?? "");
  if (!label || label.length > 72) return null;
  return { label, box: m[1] != null };
}

function trailingOptions(text: string): { options: string[]; boxed: boolean; cue: string } | null {
  const lines = visibleLines(text);
  let i = lines.length - 1;
  while (i >= 0 && !lines[i].trim()) i--;
  const hit: { label: string; box: boolean }[] = [];
  while (i >= 0) {
    const p = parseOptionLine(lines[i]);
    if (!p) {
      if (hit.length && !lines[i].trim()) {
        i--;
        continue;
      }
      break;
    }
    hit.push(p);
    i--;
  }
  hit.reverse();
  if (hit.length < 2 || hit.length > 8) return null;
  const seen = new Set<string>();
  const options: string[] = [];
  for (const h of hit) {
    if (seen.has(h.label)) continue;
    seen.add(h.label);
    options.push(h.label);
  }
  if (options.length < 2) return null;
  let cue = "";
  while (i >= 0 && !lines[i].trim()) i--;
  if (i >= 0) cue = stripMd(lines[i]);
  return { options, boxed: hit.some((h) => h.box), cue };
}

export function parseChatChoices(text: string): ChatChoices | null {
  const t = text.trim();
  if (!t) return null;
  const list = trailingOptions(t);
  if (list && (list.boxed || MULTI_RE.test(t) || LIST_CUE_RE.test(list.cue))) {
    return {
      kind: "list",
      multi: list.boxed || MULTI_RE.test(t),
      options: list.options,
    };
  }
  const lines = visibleLines(t).map((s) => stripMd(s)).filter((s) => s.length);
  const last = lines.at(-1) ?? "";
  if (YN_RE.test(last) && /[？?]/.test(last) && !FLUFF_YN_RE.test(last)) {
    return { kind: "yn", multi: false };
  }
  return null;
}

export function lastAsk(text: string): string {
  const lines = visibleLines(text)
    .map((s) => stripMd(s))
    .filter((s) => s.length);
  for (let i = lines.length - 1; i >= 0; i--) {
    const s = lines[i];
    if (YN_RE.test(s) || /[？?]/.test(s)) {
      return s.length > 160 ? s.slice(-160) : s;
    }
  }
  const t = stripMd(text).replace(/\s+/g, " ").trim();
  return t.length > 160 ? t.slice(-160) : t;
}

/** Turn a chip click into a full user reply the model can execute (bare「是」is too ambiguous). */
export function choiceReply(assistantMsg: string, label: string, choices: ChatChoices): string {
  if (choices.kind !== "yn") return label;
  const q = lastAsk(assistantMsg);
  return `${label}。针对上一问「${q}」：请立刻执行对应操作；不要重复已做过的检索/提炼，也不要再问同一句。`;
}

export function joinChosen(labels: string[]): string {
  return labels.join("、");
}
