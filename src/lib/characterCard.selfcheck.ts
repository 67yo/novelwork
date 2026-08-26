import {
  buildCharacterWholeAiInstruction,
  emptyCharacterCard,
  formatCharacterExtracted,
  listMissingCharacterFields,
  normalizeCharacterCard,
  parseCharacterCardJson,
  syncLegacyFields,
} from "@/lib/characterCard";

const legacy = normalizeCharacterCard({
  role: "刺客",
  gender: "女",
  age: "外表二十，实际三百岁",
  alignment: "影盟",
  personality: "冷",
  style: "少言",
  motto: "刀不空出",
  constraints: "旧约束",
});
console.assert(legacy.world_position.social_role === "刺客");
console.assert(legacy.world_position.faction === "影盟");
console.assert(legacy.core_belief.belief === "刀不空出");
console.assert(legacy.voice.positioning === "少言");

const rich = normalizeCharacterCard({
  gender: "男",
  age: "中年",
  aliases: "老李",
  world_position: {
    birth_class: "贱籍",
    faction: "商会",
    social_role: "账房",
    conflict_stance: "反对苛税",
  },
  world_anchors: {
    embodies_law: "等价交换",
    embodies_note: "每笔账都要还",
    shaped_by_law: "信息有价",
    shaped_by_note: "从小在情报黑市长大",
    will_challenge: "皇权专营",
  },
  relations: [
    {
      name: "张三",
      relation: "旧识",
      definition: "可利用的棋子",
      default_attitude: "热情但贪婪",
      hidden_tension: "想套出张三的秘密",
    },
  ],
  core_belief: { belief: "钱能买命", author_verdict: "disprove", source: "早年饥荒" },
  deep: {
    desire_surface: "攒够赎身银",
    desire_deep: "被真正信任",
    fear: "账本被抄",
    fear_source: "父辈抄家",
    secret_content: "藏了第二套账",
    secret_who_knows: "无",
    secret_should_know: "张三",
    secret_exposure: "满门抄斩",
    line_trigger: "动我账本",
    line_source: "职业尊严",
    line_reaction: "反杀",
    trauma_wound: "抄家夜",
    trauma_trigger: "火光",
    trauma_stress: "失语",
    trauma_imprint: "反复核对数字",
    contradiction_poles: "贪婪 vs 义气",
    contradiction_source: "江湖规矩",
    contradiction_trajectory: "择一破碎",
    arc_growth: "学会托付",
    arc_fall: "彻底冷漠",
    arc_choice: "是否交出账本",
  },
  voice: {
    positioning: "市井账房，语调平稳带刺",
    cognitive_filter: "凡事先算账",
    sentence_length: "偏短",
    pause: "算完再说",
    patterns: "反问居多",
    catchphrases: ["这账不对", "再说"],
    emotion_anger: "压低嗓音",
    emotion_tense: "语速加快",
    emotion_mask: "笑着改话题",
    emotion_sad: "沉默",
    emotion_happy: "多说半句",
    banned: "绝不说对不起",
  },
});

const md = formatCharacterExtracted(rich, "李四");
console.assert(md.includes("## 真名"));
console.assert(md.includes("李四"));
console.assert(md.includes("## 关系网络"));
console.assert(md.includes("张三"));
console.assert(md.includes("终将证伪"));
console.assert(md.includes("句法指纹"));
console.assert(md.includes("这账不对"));

const synced = syncLegacyFields(rich);
console.assert(synced.role === "账房");
console.assert(synced.alignment === "商会");
console.assert(synced.constraints.includes("世界位置"));

const parsed = parseCharacterCardJson(
  '杂音\n{"gender":"女","aliases":"小雪","世界位置":{"出生阶层":"贵族"},"core_belief":{"belief":"血脉即责任","author_verdict":"悬而未决"}}\n',
);
console.assert(parsed?.gender === "女");
console.assert(parsed?.aliases === "小雪");
console.assert(parsed?.world_position.birth_class === "贵族");
console.assert(parsed?.core_belief.author_verdict === "unresolved");

console.assert(formatCharacterExtracted(emptyCharacterCard()) === "");
console.assert(listMissingCharacterFields(rich, "李四").length === 0);
console.assert(listMissingCharacterFields(emptyCharacterCard(), "").length > 10);
const inst = buildCharacterWholeAiInstruction("改成腹黑账房", {
  trueName: "李四",
  lawOptions: ["等价交换"],
});
console.assert(inst.includes("每一个字段都必须"));
console.assert(inst.includes("等价交换"));
console.log("characterCard.selfcheck ok");
