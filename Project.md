# Nove Work — 章节 AI 策略（生成 / 预生成 / 精修）

本文是章节相关 AI 流程的**单一事实来源**：大纲生成、正文预生成、精修、记忆抽取。改代码时必须同步改本文。

**维护约定（强制）**

凡改动下列任一内容，**同一变更内**更新本文对应章节：

- 命令入口、流程步骤、取消/进度阶段  
- Prompt 约束、输入材料、冲突优先级  
- **大纲 brief 材料**（根人物/剧情/知识卡、绑定知识库检索、`assemble_outline_gen_brief` / `root_linked_plots_and_knowledge` / `build_canon_context`）  
- 字数规则、落地检查、记忆抽取时机  
- 根节点「游戏设定」绑定 / 同步设定卡  
- 根节点 / Chat 斜杠 / UI 行为差异  

实现入口总表：

| 区域 | 路径 |
|------|------|
| 命令 | `src-tauri/src/commands.rs` |
| Prompt | `src-tauri/src/prompts.rs` |
| 落地检查 | `src-tauri/src/chapter_constraints.rs` |
| 章节记忆 | `src-tauri/src/chapter_memory.rs` + SQLite / Lance |
| UI | `src/views/WorkspaceView.vue` |
| 设置模型 | `generate_model` / `refine_model` / `chat_model` / `knowledge_model` |

可取消：凡走 `arm_chat_cancel(novel_id)` 的流程均可 `chat_cancel` 中止（含落地补写、大纲生成、记忆抽取）。

进度事件：`chapter-progress`（预生成 / 精修 / 根节点生成章节卡 / 生成下一章 / 手动重抽记忆）。

---

## 0. 概念区分

| 名称 | 产出 | 典型入口 |
|------|------|----------|
| **章节卡生成** | 结构树上的章节节点：`label` + `outline`（**无正文**） | 根节点「生成章节卡」、Chat `/章节卡`、章节卡「生成下一章」 |
| **预生成** | 单章 **正文** Markdown | 章节卡「预生成」 |
| **精修** | 在已有正文上小幅改写（**不**抽记忆） | 章节卡「精修」 |
| **章节记忆** | 摘要事实（非整章正文） | 记忆面板**手动**提取（可写注意事项） |

---

## 1. 章节卡生成（大纲 AI）

目标：按全书总章数节奏，为指定编号写**标题 + 大纲要点**，写入结构树；**不写正文、不抽记忆**。

输出契约（各路径共用）：模型回复须含 JSON  

`{"outlines":[{"n":1,"label":"第一章 · …","outline":"…"}]}`  

代码解析后 `apply_chapter_outlines`；同号处理见各路径模式。

### 1.0 须考虑材料（左侧入口共用）

左侧「生成章节卡」「生成下一章」「刷新大纲」在调用 LLM 前：

1. `preview_chapter_outline_brief` 组装可编辑 brief（`outline_gen_brief`）。  
2. UI 弹出确认框：展示全部材料 +「对本批的期望」占位，用户可改。  
3. 确认后把编辑后的 `user_brief` 原样交给 `generate_chapter_cards` / `plan_next_chapters` / `regenerate_chapter_outline`。  

**组装规则（`assemble_outline_gen_brief`）**

| 情形 | 材料 |
|------|------|
| **根参考（始终）** | 见下表「根参考明细」 |
| 仅根节点（生成第 1 章起） | 根参考 |
| 已有前序章（编号 &lt; 本批 from） | 根参考 + 各前序章**大纲 + 章节记忆**（`build_refine_memory_context`） |
| 生成下一章 | 上项且 **含当前章**（from = 当前章号+1 ⇒ 编号 &lt; from）+ 可选当前章正文截断；user 另附 `knowledge_strategy` |
| 刷新本章大纲 | 根参考 + 编号 &lt; 本章的大纲/记忆 + 当前旧大纲（标注将被覆盖）+ 用户期望 |

Chat `/章节卡` 无确认框；`user_brief` 为空时后端按同一规则自动组装。

#### 1.0.1 根参考明细（大纲 AI 必含）

组装入口：`assemble_outline_gen_brief`。

| 块 | 函数 / 来源 | 内容 | 大纲侧约束 |
|----|-------------|------|------------|
| 简介 + 根说明 + 人物 | `root_generate_reference` | `synopsis`、根 `outline`、根链接/边关联的**人物卡** | 定调与人设 |
| **剧情卡** | `root_linked_plots_and_knowledge` | 根 `linked_side_plot_ids` + 根边上的 `side_plot`；只注入**要点**（`outline`），不塞剧情卡全文 | **本批须为其安排合理推进，禁止只点名** |
| **知识卡** | 同上 | 根 `linked_knowledge_ids` + 根边上的 `knowledge`；优先 `extracted`（截断），否则提取需求 / 节点 outline | **专名与规则勿与之冲突** |
| **绑定公共知识库** | `build_canon_context`（当 `novel.knowledge_ids` 非空） | 根上 `from_canon` 设定卡摘要 + 绑定库关键词检索片段；文案随 `canon_mode`（reference / strict） | strict：**硬设定**，禁止发明冲突设定；reference：参考、勿明显矛盾 |

说明：

- 预生成正文另有 `chapter_context`（含根贯穿卡）；**故意不**把剧情/知识并进 `root_generate_reference`，以免预生成 user 里重复两份。大纲路径单独拼 `root_linked_plots_and_knowledge`。  
- Prompt：`gen_chapter_cards_system` / `plan_next_chapters_system` 须写明「根剧情卡须推进；知识卡/绑定设定勿冲突」。  

### 1.1 根节点「生成章节卡」

| 项 | 说明 |
|----|------|
| 预览 | `preview_chapter_outline_brief(novel_id, "root", count)` |
| 命令 | `generate_chapter_cards(novel_id, count, user_brief)` |
| Prompt | `gen_chapter_cards_system` / `gen_chapter_cards_user`（user 注入编辑后的 brief） |
| 模型 | `chat_model` |
| 范围 | 第 **1–count** 章；`count` clamp 到 `1..=100`，且若 `novel.chapter_count > 0` 则不超过全书章数 |
| 冲突模式 | **Overwrite**：同号覆盖标题与大纲；无则新建并挂到章节链 |
| AI 策略 | 按总章数分配本批位置；章间衔接；不得推翻 brief 中既定事实；**推进根剧情卡**；**遵守根知识卡与绑定设定**；尊重用户期望 |
| UI | 根节点左侧：数量 → 确认框 → 生成；进度 3 步：context → planning → saving |

### 1.2 Chat `/章节卡`

| 项 | 说明 |
|----|------|
| 解析 | `parse_gen_chapter_cards`：`/章节卡 1-10`、`/章节卡 覆盖\|跳过\|强制追加 3-5` 等 |
| 实现 | 与根节点共用 `apply_llm_chapter_cards`（自动组装 brief，无弹窗；材料规则同 §1.0） |
| 默认模式 | 未指定模式且无冲突 → **ForceAppend**；有冲突且未指定模式 → **先提示**，不调用 LLM |
| 模式 | Overwrite / Skip / ForceAppend |
| 进度 | `chat-progress`（thinking / apply_outlines / saving） |

### 1.3 章节卡「生成下一章」

| 项 | 说明 |
|----|------|
| 预览 | `preview_chapter_outline_brief(novel_id, "plan_next", count, node_id)` |
| 命令 | `plan_next_chapters(novel_id, node_id, count, user_brief)` |
| Prompt | `plan_next_chapters_system` / `plan_next_chapters_user` |
| 模型 | `chat_model` |
| 范围 | 当前章编号之后连续 `count` 章（1–12），且不超过全书计划章数 |
| 冲突模式 | **Overwrite** |
| 输入材料 | 见 §1.0（含根剧情卡/知识卡/绑定库）；另附 `knowledge_strategy` |
| AI 策略 | 承接当前局势与期望；不得推翻记忆；多章递进；**根剧情卡须推进**；**知识卡/绑定设定勿冲突**；只输出本批编号 |
| UI | 章节卡按钮 + 数量 → 确认框 → 生成；进度 chapter-progress |

### 1.3a 章节卡「刷新大纲」

| 项 | 说明 |
|----|------|
| 预览 | `preview_chapter_outline_brief(novel_id, "regen_outline", 1, node_id)` |
| 命令 | `regenerate_chapter_outline(novel_id, node_id, user_brief)` |
| 实现 | 共用 `apply_llm_chapter_cards`（与根节点「生成章节卡」同源） |
| Prompt | `gen_chapter_cards_system` / `gen_chapter_cards_user`（单章、`replace=true`） |
| 模型 | `chat_model` |
| 范围 | **仅当前章**编号；冲突模式 **Overwrite**（覆盖标题+大纲） |
| 输入材料 | 见 §1.0「刷新本章大纲」（含根剧情卡/知识卡/绑定库） |
| 不改 | 正文、章节记忆、剧情卡关联 |
| UI | 章节大纲旁「刷新大纲」→ 确认框 → 生成；进度 chapter-progress |

### 1.3b 章节卡「生成剧情卡」

| 项 | 说明 |
|----|------|
| 命令 | `generate_chapter_plots(novel_id, node_id, outline, user_notes)` |
| 实现 | 共用 `enrich_plots_for_chapter_node`（Chat `/剧情卡`、`/完善剧情` 同源） |
| Prompt | `gen_chapter_plots_system` / `gen_chapter_plots_user`（含用户补充剧情） |
| 模型 | `chat_model` |
| 产出 | 多张剧情卡挂到本章 + 补齐/关联人物卡（追加，不删已有） |
| UI | 与「生成下一章」同行；**两步确认**：① 可编辑本章大纲 + 可选自定义剧情 → 继续；② 最终确认 → 生成 |
| 进度 | chapter-progress：context → gen_plots → saving |

### 1.4 章节卡生成检查清单

改大纲 / 剧情卡生成时：

1. [ ] 更新 `preview_chapter_outline_brief` / `generate_chapter_cards` / `plan_next_chapters` / `regenerate_chapter_outline` / `generate_chapter_plots` / Chat（若涉及）  
2. [ ] 更新 `assemble_outline_gen_brief` / `root_linked_plots_and_knowledge` / `build_canon_context`（若改材料）  
3. [ ] 更新 `outline_gen_brief` / `gen_chapter_cards_*` / `plan_next_chapters_*` / `gen_chapter_plots_*`  
4. [ ] **同步本文 §1.0 / §1.0.1**（含根剧情卡、知识卡、绑定库规则）  
5. [ ] UI 文案：`messages.ts` 全语言  

---

## 2. 章节预生成（正文 Generate）

### 2.1 目的

在强约束下写出**本章完整正文**（Markdown）。篇幅以**树根节点每章目标字数**为准（见 §4）。

### 2.2 输入材料（冲突优先级）

| 材料 | 来源 | 约束角色 |
|------|------|----------|
| 全书简介 + 根节点说明 | `novel.synopsis`、根 `outline` | 定调 |
| 根节点关联人物卡 | 根链接 / 边 | 全书人设 |
| **前序大纲 + 章节记忆** | 结构树前序章 + `chapter_memory` | **既定事实，禁止推翻** |
| **本章大纲** | 章节节点 `outline` | **主线，不可丢关键节拍** |
| **链接剧情卡**（含根贯穿） | 链接 / 边 | **须落地并推进** |
| 链接人物卡 + 关系边 | 本章 / 剧情卡 / 根并集 | 言行合卡 |
| 链接知识卡 | 提取特征 | 技法/设定边界 |
| **绑定公共知识库** | `novel.knowledge_ids` + `canon_mode` | **reference**：参考；**strict**：硬设定，检索注入且冲突优先 |
| 知识库策略 | `knowledge_strategy` | 补充 |
| 全书计划章数 | `chapter_count` | 本章信息量与悬念 |
| **每章目标字数** | §4 | **硬性篇幅（写进 Prompt）** |

冲突时（`canon_mode=reference`）：**前序记忆/大纲 → 本章大纲 → 剧情卡 → 人物 → 知识卡 → 简介**。  
冲突时（`canon_mode=strict`）：**前序记忆/大纲 → 绑定知识库设定 → 本章大纲 → 剧情卡 → 人物 → 知识卡 → 简介**。未链接设定勿硬塞。

### 2.2b 根节点「游戏设定」绑定

| 项 | 说明 |
|----|------|
| 字段 | `knowledge_ids`（绑定书）、`canon_mode`（`reference`/`strict`）、`knowledge_strategy` |
| 保存 | `update_novel_canon` |
| 注入（预生成 / 精修 / **大纲 brief**） | `build_canon_context`：根上 `from_canon` 设定卡摘要 + 绑定库关键词检索片段 |
| 同步设定卡 | `sync_canon_settings`：每本绑定库提炼一张 `from_canon` 知识卡挂根（同书覆盖） |
| 设定对齐 | 仅正文预生成：strict 且有检索材料时，大纲涉及的设定词未出现在正文 → `repair_canon_*` 一轮 |
| 「禁止发明冲突设定」 | 不得自创与绑定库/设定卡矛盾的专名、规则、体系、禁忌；允许在设定范围内写剧情与润色表述 |

### 2.3 流程

命令：`preview_generate_chapter` → 确认框 → `generate_chapter(novel_id, node_id, memory_node_ids, user_brief)` · Prompt：`generate_chapter_*` · 模型：`generate_model`

1. **确认框**（手动点预生成）：左勾选前序章（默认勾选已有记忆者）；右编辑预生成条件（`localStorage` `novework.generateBrief`，可重置出厂缺省）。  
2. 组装 `root_generate_reference` + **勾选章**大纲/记忆（`build_refine_memory_context`）+ `chapter_context` + 用户条件。  
3. `root_chapter_word_target` 解析字数；写入 system/user（目标 + ±60）。  
4. 代码生成「必须落地」契约（大纲节拍 / 剧情卡 / 本章焦点人物）写入 user。  
5. 首轮 LLM 写正文。  
6. **落地补写**（可选）：关键词未命中 → `repair_chapter_*` 再一轮（带字数约束）；Mock 跳过。  
7. **设定对齐**（可选，`canon_mode=strict`）：大纲涉及的绑定设定词未命中 → `repair_canon_*`。  
8. 写 `chapters/{node_id}.md`（附 footer），树上字数按正文（不含 footer）。偏目标仅 `word_count_off_note`。  
9. **不**抽取章节记忆。  

一键自动：跳过确认框，默认勾选有记忆的前序章 + 已存/缺省条件。  

进度阶段（3）：`context` → `writing` → `repair_beats`（校验/必要时补写；strict 设定对齐同阶段内完成）。

### 2.4 落地检查（`chapter_constraints`）

- 大纲节拍、剧情卡要点、本章焦点人物姓名关键词命中  
- `ponytail:` 关键词 ≠ 语义；误报高时再考虑 LLM 评审  

### 2.5 预生成检查清单

1. [ ] `generate_chapter` / `generate_chapter_*` / `repair_*` / `chapter_constraints`  
2. [ ] **同步本文 §2、§4**  
3. [ ] i18n（若 UI 变）  

---

## 3. 章节精修（Refine）

### 3.1 目的

**不大改剧情**：对照历史与本章设定，小幅梳理衔接、因果、人物、结构、语句；篇幅仍落在根节点目标 ±60。**不**自动抽取章节记忆。

### 3.2 输入材料

| 材料 | 说明 |
|------|------|
| **模式 A：记忆+大纲**（默认） | 前 N 章大纲 + 章节记忆；同书关键词补检索；**不注入**前序全文 |
| **模式 B：章节全文** | 前 N 章标题 + 大纲 + **完整正文** |
| 当前章链接设定 | 同预生成 `chapter_context` |
| 当前章已生成正文 | 精修对象；尽量保留骨架与段落顺序 |
| **精修条件**（可编辑） | 用户补充侧重点；持久化为默认；「重置条件」恢复出厂缺省；注入 `refine_chapter_user` |
| **每章目标字数** | §4；system/user 写明具体区间 |

UI：点「精修」→ 确认面板编辑条件 → 开始；`mode` = `memory` \| `full` 与前 N 章仍在面板外设置。一键自动跳过确认、用已存条件。

### 3.3 流程

命令：`refine_chapter(…, user_brief)` · Prompt：`refine_chapter_*` · 模型：`refine_model`

1. 按模式组装前 N 章材料 + `chapter_context` + 当前正文 + 用户精修条件；若绑定了知识库，将 `build_canon_context` 并入策略材料。  
2. 字数目标写入 Prompt；**无**额外篇幅校准轮次。  
3. 一轮 LLM 精修。  
4. 写回正文、更新树上字数。  
5. 文末修正点分类：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。偏目标仅完成消息标注。  

进度阶段（2）：`context` → `refining`。

### 3.4 策略要点

1. 历史只读：禁止改写历史章。  
2. 剧情卡缺漏可**小幅补写**，禁止另起炉灶。  
3. 改动克制：只改矛盾、不合理、不通顺、未落地、字数超差处。  
4. 用户精修条件须遵守，但仍不大改剧情。  
5. 建议 `refine_model` 用更强模型。  

### 3.5 模式对比

| | 记忆+大纲 | 章节全文 |
|--|-----------|----------|
| Token | 省 | 贵，N 大易顶满 |
| 细节 | 依赖抽取质量 | 最全 |
| 适用 | 日常、章数多 | 需逐句对照前几章 |

### 3.6 精修检查清单

1. [ ] `refine_chapter` / `refine_chapter_*` / 记忆抽取  
2. [ ] **同步本文 §3、§4、§5**  
3. [ ] i18n（若 UI 变）  

---

## 4. 硬性篇幅（预生成 / 精修共用）

| 项 | 规则 |
|----|------|
| 目标来源 | 树根 `word_count_min` / `word_count_max` → 否则 `novel` 同字段 → 默认 2000–3000 |
| 计数 | 非空白字符（`count_words`） |
| 误差 | ≤60：有效 `[wmin−60, wmax+60]`；min=max 则为单点 ±60 |
| Prompt | system/user 写明目标与有效区间；偏短续写、偏长删冗 |
| 代码 | **不做**第二轮篇幅校准 LLM；仅 `word_count_off_note` |
| 常量 | `WORD_COUNT_TOLERANCE = 60`（`commands.rs`） |

---

## 5. 章节记忆库

- **内容**：非完整正文。① 整体情节约 150–400 字；② 要点（人物/行为/目标/承诺/人设）。禁止照抄大段对话描写。  
- **存储**：按 `novel_id` + `node_id` → SQLite `chapter_memory` + Lance `chapter_memory`；与公共知识库分离。  
- **抽取时机**：**仅手动**（预生成 / 精修均不自动抽）。命令：`regenerate_chapter_memory(novel_id, node_id, user_notes, memory_node_ids)`；模型：`knowledge_model`。  
- **注意事项**：`localStorage`（`novework.memoryExtractNotes`）保存用户改过的条件并作为默认；首次为空时填入出厂缺省（`memoryExtractNotesDefault`）。「重置条件」写回出厂缺省。全文作为 `user_notes` 写入 `extract_chapter_memory_user`；系统 prompt 只定角色/底线。  
- **其他章记忆对照**：面板左侧勾选其他章节（默认勾选已有记忆者）；其记忆注入 prompt 仅供去重/衔接对照，**禁止**抄入本章记忆。无记忆的勾选不注入。注意事项里可写如何对照。  
- **UI**：工具栏一个「章节记忆」入口；宽面板横排「对照用其他章记忆 | 已提取记忆 | 抽取注意事项」，标题旁「重置条件」，底部提取按钮。  
- **管理**：全部章节记忆面板可删单条/清整章（`set_chapter_memory`）；删章/删书清理。  
- **检索**：关键词重叠打分（`ponytail:` 日后可换真向量）。  
- **冷启动**：无记忆时精修仍可用大纲；需记忆时在面板手动提取。  

---

## 6. 相关 UI 与自动化

- **设置**：预生成 / 精修 / Chat / 知识（记忆抽取）模型分栏。  
- **根节点**：封面、每章字数、全书章数、**游戏设定绑定**（知识库多选 + 严格/参考 + 策略 + 同步设定卡）、根大纲、**生成章节卡（数量 → 确认材料/期望）**。  
- **章节卡**：一行「生成剧情卡（两步确认）| 预生成（记忆章勾选 + 条件）| 精修（条件确认面板）」；精修参考模式 + 前 N；大纲旁「刷新大纲」（确认材料后覆盖标题+大纲）；生成下一章（数量 → 确认材料/期望）；停止、**手动编辑正文**（`save_chapter`）、复制、**章节记忆**（面板内手动提取）。  
- **画布**：添加卡片、一键排版、查看全部章节记忆。  
- **一键自动**：按章顺序预生成 → 精修，可中止并 cancel 在途 LLM。  
- **等待界面**：阶段文案 + 进度条 + 每步耗时。  

---

## 7. 总检查清单

| 改动类型 | 必须更新 |
|----------|----------|
| 章节卡 / 大纲生成 | §1（含 **§1.0.1** 根剧情卡/知识卡/绑定库） |
| 预生成正文 | §2、§4 |
| 游戏设定 / 知识库绑定 | §2.2b（并核对 §1.0.1 大纲注入） |
| 精修 | §3、§4 |
| 字数规则 | §4 + `WORD_COUNT_TOLERANCE` |
| 记忆抽取/存储（手动、注意事项） | §5 |
| UI 文案 | `src/i18n/messages.ts` 六语言 |

Cursor 规则：`.cursor/rules/project-md-sync.mdc`（改上述流程时强制同步本文）。
