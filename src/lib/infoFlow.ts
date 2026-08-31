/** 信息传播结构化（slot=wv_info_flow） */
export type InfoFlowData = {
  premise: string;
  info_speed: string;
  info_barrier: string;
  rumor_truth: string;
  knowledge_carrier: string;
};

/** 读盘/LLM 可能仍带旧键 message_truth（留言→流言 笔误）。 */
type InfoFlowInput = Partial<InfoFlowData> & { message_truth?: string };

export function emptyInfoFlow(): InfoFlowData {
  return {
    premise: "",
    info_speed: "",
    info_barrier: "",
    rumor_truth: "",
    knowledge_carrier: "",
  };
}

export function normalizeInfoFlow(
  raw: InfoFlowInput | null | undefined,
): InfoFlowData {
  const d = emptyInfoFlow();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.info_speed = raw.info_speed ?? "";
  d.info_barrier = raw.info_barrier ?? "";
  d.rumor_truth = raw.rumor_truth ?? raw.message_truth ?? "";
  d.knowledge_carrier = raw.knowledge_carrier ?? "";
  return d;
}

export function formatInfoFlowExtracted(d: InfoFlowData): string {
  const lines: string[] = [];
  const premise = d.premise.trim();
  if (premise) {
    lines.push("## 一句话立意", premise, "");
  }
  const speed = d.info_speed.trim();
  if (speed) {
    lines.push("## 信息速度", speed, "");
  }
  const barrier = d.info_barrier.trim();
  if (barrier) {
    lines.push("## 信息壁垒", barrier, "");
  }
  const rt = d.rumor_truth.trim();
  if (rt) {
    lines.push("## 流言与真相", rt, "");
  }
  const carrier = d.knowledge_carrier.trim();
  if (carrier) {
    lines.push("## 知识载体", carrier, "");
  }
  return lines.join("\n").trim();
}
