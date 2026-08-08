/** Pretty-print JSON lines in chat replies for readable display (does not change stored text). */
export function formatChatContent(text: string): string {
  if (!text) return text;
  return text
    .split("\n")
    .map((line) => {
      const t = line.trim();
      if (!(t.startsWith("{") || t.startsWith("["))) return line;
      try {
        return JSON.stringify(JSON.parse(t), null, 2);
      } catch {
        return line;
      }
    })
    .join("\n");
}
