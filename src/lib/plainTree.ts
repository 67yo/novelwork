import type { NovelTree } from "@/lib/api";

/** 去掉 Vue 响应式 / 不可序列化字段，供 Tauri IPC 落盘 */
export function plainTree(tr: NovelTree): NovelTree {
  const plain = JSON.parse(JSON.stringify(tr)) as NovelTree;
  if (!plain.novel_id?.trim()) {
    throw new Error("invalid tree: missing novel_id");
  }
  if (!Array.isArray(plain.nodes) || !Array.isArray(plain.edges)) {
    throw new Error("invalid tree: nodes/edges");
  }
  return plain;
}
