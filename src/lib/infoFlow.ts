/** 信息传播结构化（slot=wv_info_flow） */
export type InfoFlowData = {
  premise: string;
  info_speed: string;
  info_barrier: string;
  message_truth: string;
  knowledge_carrier: string;
};

export function emptyInfoFlow(): InfoFlowData {
  return {
    premise: "",
    info_speed: "",
    info_barrier: "",
    message_truth: "",
    knowledge_carrier: "",
  };
}

export function normalizeInfoFlow(
  raw: Partial<InfoFlowData> | null | undefined,
): InfoFlowData {
  const d = emptyInfoFlow();
  if (!raw) return d;
  d.premise = raw.premise ?? "";
  d.info_speed = raw.info_speed ?? "";
  d.info_barrier = raw.info_barrier ?? "";
  d.message_truth = raw.message_truth ?? "";
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
  const mt = d.message_truth.trim();
  if (mt) {
    lines.push("## 留言与真相", mt, "");
  }
  const carrier = d.knowledge_carrier.trim();
  if (carrier) {
    lines.push("## 知识载体", carrier, "");
  }
  return lines.join("\n").trim();
}
