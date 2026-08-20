/** ponytail: parseChatChoices. Run: npx tsx src/lib/chatChoices.selfcheck.ts */
import { choiceReply, joinChosen, lastAsk, parseChatChoices } from "./chatChoices.ts";

function eq(a: unknown, b: unknown, msg: string) {
  if (JSON.stringify(a) !== JSON.stringify(b)) {
    throw new Error(`${msg}: ${JSON.stringify(a)} !== ${JSON.stringify(b)}`);
  }
}

eq(parseChatChoices("需要我帮你生成这一章吗？"), { kind: "yn", multi: false }, "yn");
eq(
  parseChatChoices("请选择：\n1. 预生成\n2. 精修\n3. 只改大纲"),
  { kind: "list", multi: false, options: ["预生成", "精修", "只改大纲"] },
  "single list",
);
eq(
  parseChatChoices("可多选：\n- 人物\n- 剧情\n- 知识卡"),
  { kind: "list", multi: true, options: ["人物", "剧情", "知识卡"] },
  "multi keyword",
);
eq(
  parseChatChoices("勾选：\n- [ ] 章纲\n- [ ] 正文"),
  { kind: "list", multi: true, options: ["章纲", "正文"] },
  "checkboxes",
);
eq(parseChatChoices("这一章已经写完了。"), null, "plain");
eq(
  parseChatChoices("已完成：\n1. 创建人物卡\n2. 挂到根节点"),
  null,
  "status numbered list is not a choice",
);
eq(
  parseChatChoices("已挂上知识卡。若还要改文风可以说一声。"),
  null,
  "done, no yn chips",
);
eq(parseChatChoices("任务 1 已完成。要不要继续下一项？"), null, "fluff continue yn");
eq(parseChatChoices("是否继续执行剩余任务？"), null, "fluff是否继续");
eq(joinChosen(["人物", "剧情"]), "人物、剧情", "join");
eq(
  lastAsk("已整理修炼设定。\n需要我把这份提炼写成知识卡（挂在当前小说下）吗？"),
  "需要我把这份提炼写成知识卡（挂在当前小说下）吗？",
  "lastAsk",
);
{
  const msg = "需要我把这份提炼写成知识卡（挂在当前小说下）吗？";
  const reply = choiceReply(msg, "是", { kind: "yn", multi: false });
  if (!reply.startsWith("是。") || !reply.includes("写成知识卡") || !reply.includes("不要重复")) {
    throw new Error(`choiceReply: ${reply}`);
  }
  eq(choiceReply(msg, "预生成", { kind: "list", multi: false, options: ["预生成"] }), "预生成", "list passthrough");
}

console.log("chatChoices.selfcheck ok");
