/** LLM 常把文本写成对象/数组；落盘前收成字符串，避免 format 里 .trim 炸掉。 */
export function asText(v: unknown): string {
  if (typeof v === "string") return v;
  if (v == null) return "";
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  if (Array.isArray(v)) return v.map(asText).filter(Boolean).join("\n");
  if (typeof v === "object") {
    return Object.values(v as Record<string, unknown>).map(asText).filter(Boolean).join("\n");
  }
  return "";
}
