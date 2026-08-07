/** ponytail: stripChapterMeta；node src/lib/md.selfcheck.mjs */
function stripChapterMeta(src) {
  let s = src.replace(/\r\n/g, "\n").trim();
  if (!s) return "";
  s = s.replace(/\n*---\s*\n>\s*(?:节点约束校验摘要|Constraint check)[\s\S]*$/i, "").trim();
  s = s.replace(/\n*（请在设置页填写[\s\S]*?）\s*$/u, "").trim();
  s = s.replace(/\n*\(Please (?:set|configure)[\s\S]*?\)\s*$/i, "").trim();
  return s;
}

const cases = [
  [
    "# 第一章 夜色\n\n主角推开门。\n\n---\n> 节点约束校验摘要：已按大纲生成。章节节点 `abc`。模型：m。\n",
    "# 第一章 夜色\n\n主角推开门。",
  ],
  [
    "para only\n\n---\n> Constraint check: ok. Node `x`. Model: y.\n",
    "para only",
  ],
];

for (const [input, want] of cases) {
  const got = stripChapterMeta(input);
  if (got !== want) {
    console.error("FAIL", { want, got });
    process.exit(1);
  }
}
console.log("ok");
