import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

/** 两个全角空格 = 中文段首缩进两字；写入文本，复制时保留 */
const PARA_INDENT = "\u3000\u3000";

/** 去掉 AI/系统反馈，保留标题与正文 Markdown。 */
export function stripChapterMeta(src: string): string {
  let s = src.replace(/\r\n/g, "\n").trim();
  if (!s) return "";
  // app 追加页脚：---\n> 节点约束校验摘要… / Constraint check…
  s = s.replace(/\n*---\s*\n>\s*(?:节点约束校验摘要|Constraint check)[\s\S]*$/i, "").trim();
  // mock 或模型偶发的文末说明行
  s = s.replace(/\n*（请在设置页填写[\s\S]*?）\s*$/u, "").trim();
  s = s.replace(/\n*\(Please (?:set|configure)[\s\S]*?\)\s*$/i, "").trim();
  return s;
}

/** 章节 Markdown → HTML；单换行按段落断开；段首写入全角空格以便复制保留缩进。 */
export function renderChapterMd(src: string): string {
  const text = src.replace(/\r\n/g, "\n").trim();
  if (!text) return "";
  // ponytail: 单换行→段落；列表密集的 Markdown 会拆坏，章节正文够用
  const normalized = text.replace(/\n{3,}/g, "\n\n").replace(/([^\n])\n([^\n])/g, "$1\n\n$2");
  const html = marked.parse(normalized, { async: false }) as string;
  return withCopyableParaIndent(html);
}

/** 段首缩进写入文本节点（CSS text-indent 复制丢失）。 */
export function withCopyableParaIndent(html: string): string {
  return html.replace(/<p([^>]*)>(?:\u3000)*/gi, `<p$1>${PARA_INDENT}`);
}
