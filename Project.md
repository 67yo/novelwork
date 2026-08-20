# Novel Work — 章节 AI 策略（生成 / 预生成 / 精修）

本文是章节相关 AI 流程的**单一事实来源**：大纲生成、正文预生成、精修、记忆抽取。改代码时必须同步改本文。

**维护约定（强制）**

凡改动下列任一内容，**同一变更内**更新本文对应章节：

- 命令入口、流程步骤、取消/进度阶段  
- Prompt 约束、输入材料、冲突优先级  
- **大纲 brief 材料**（根人物/剧情/知识卡、`assemble_outline_gen_brief` / `root_linked_plots_and_knowledge`）  
- 字数规则、落地检查、记忆抽取时机  
- 根节点 / 全局 Chat / UI 行为差异  

实现入口总表：

| 区域 | 路径 |
|------|------|
| 命令 | `src-tauri/src/commands.rs` |
| Prompt | `src-tauri/src/prompts.rs` |
| 落地检查 | `src-tauri/src/chapter_constraints.rs` |
| 章节记忆 | `src-tauri/src/chapter_memory.rs` + SQLite / Lance |
| 知识检索预算 | `src-tauri/src/kb_context.rs`（`retrieve_knowledge` / 本地 MiniLM embeddings） |
| UI | `src/views/WorkspaceView.vue` |
| 设置模型 | `generate_model` / `refine_model` / `chat_model` / `knowledge_model` |

可取消：凡走 `arm_chat_cancel(novel_id)` 的流程均可 `chat_cancel` 中止（含落地补写、大纲生成、记忆抽取）。

进度事件：`chapter-progress`（预生成 / 精修 / 根节点生成章节卡 / 生成下一章 / 手动重抽记忆）。

---

## 0. 概念区分

| 名称 | 产出 | 典型入口 |
|------|------|----------|
| **章节卡生成** | 结构树上的章节节点：`label` + `outline`（**无正文**） | 根节点「生成章节卡」（空卡，无 AI）；大纲刷新等仍可由后端命令支持 |
| **分卷（volume）** | 可选中间层节点：根 → 分卷 → 章节；亦可章直接挂根 | 画布「添加分卷」/ MCP `add_volume` |
| **预生成** | 单章 **正文** Markdown；**禁止参考本章已有正文**，按大纲/卡/前序记忆重写 | 章节卡工具栏「预生成」 |
| **精修** | **必须在当前已有正文上**小幅改写（无正文则拒绝；**不**抽记忆） | 章节卡工具栏「精修」 |
| **章节记忆** | 摘要事实（非整章正文） | 记忆面板**手动**提取（可写注意事项） |

**分卷继承（写章 / MCP / 左栏）**

- 分卷：根设定 ∪ 分卷本机（人物 / 剧情 / 知识）。  
- 章节：根 ∪ **父分卷**（若有）∪ 本章本机。  
- 本章剧情**排序**仅含本章本机；根 / 卷继承只读展示。  
- 旧书无 `volume` 节点 → 行为与原先一致。  
- 删除分卷：只删分卷节点与边，卷下章节**改挂到根**，不删正文。

---

## 1. 章节卡生成（大纲 AI）

目标：按全书总章数节奏，为指定编号写**标题 + 大纲要点**，写入结构树；**不写正文、不抽记忆**。

输出契约（各路径共用）：模型回复须含 JSON  

`{"outlines":[{"n":1,"label":"第一章 · …","outline":"…"}]}`  

**`outline` 正文固定格式（硬约束，生成章节卡 / 刷新本章大纲 / 根刷新大纲共用）**

```
1. [本章定位] …
2. [开篇钩子] …
3. …
4. …   （第 3 项起由 AI 补充关键情节与信息，至少 1 条，通常 3–6 条）
```

- 第 1、2 行标签必须分别为 `[本章定位]`、`[开篇钩子]`（英文 UI 为 `[Chapter role]` / `[Opening hook]`）。  
- 多行编号条目；禁止无编号长段散文。  
- Prompt：`chapter_outline_body_format_rule`（注入 `gen_chapter_cards_*` / `plan_next_chapters_*`）。

**本章链接卡（硬约束）**

- 材料含「本章已链接人物与剧情」时：**严格遵守**链接人物人设与链接剧情卡要点（进行中须推进并写入大纲条目；已收束/已吸收/搁置作既定事实）。  
- 禁止另起无关主线、禁止只点名不落地。  
- Prompt：`chapter_linked_cards_hard_rule`；单章路径由 `append_regen_outline_extras` / `apply_llm_chapter_cards` 自动注入链接卡材料。

代码解析后 `apply_chapter_outlines`；同号处理见各路径模式。

**跨模型输出统一（硬约束）**

- Prompt 契约：`outlines_json_output_contract` — 整段只能是 `{"outlines":[{"n":number,"label":string,"outline":string}]}`。  
- 请求侧：`response_format=json_object` + 低温（0.2）。  
- 解析侧：兼容中文键 / 裸数组 / `n` 字符串 / `outline` 数组；失败则 `repair_outlines_json_*` 再请求一次收成标准 JSON。  
- 共享入口：`apply_llm_chapter_cards` / `extract_outlines_list_or_repair`。

### 1.0 须考虑材料（左侧入口共用）

左侧「生成章节卡」在调用 LLM 前：

1. `preview_chapter_outline_brief` **只读预览**材料（`outline_gen_brief` / `assemble_outline_gen_brief`）。  
2. UI 确认框：**只读材料摘要（可折叠）** + **可编辑「对本批的期望」**；确认时只把期望（可空）作为 `user_brief` 交给后端。  
3. 后端若 `user_brief` 为空或仅为短期望，则 `assemble_outline_gen_brief` + `merge_expectation_into_assembled`；若客户端仍传入整份旧式 brief（兼容），则原样使用。  

「刷新本章大纲」见 **§1.3 / §1.3a**：Chat 发送指令；材料由服务端组装，**不**随消息粘贴。

**注入预算（`kb_context`）**

| 块 | 上限 |
|----|------|
| 知识卡 `extracted`（单卡） | 500 字 |
| 根知识卡合计 | 2k |
| 公共库检索（知识卡填充 / Chat `search_knowledge`） | 召回约 **20** ∪ 规则重排 → top **5** × **400**（带 `book_id#idx`）；评测见 `tests/fixtures/kb_retrieve_cases.json` |
| 章节记忆（勾选范围内） | top **6** × **300**，整块硬顶 **~2k**（带 `memory:node_id`） |
| 本章知识卡合计 | 2k |

**组装规则（`assemble_outline_gen_brief`）**

| 情形 | 材料 |
|------|------|
| **根参考（始终）** | 见下表「根参考明细」 |
| 仅根节点（生成第 1 章起） | 根参考 |
| 已有前序章（编号 &lt; 本批 from） | 根参考 + 各前序章**大纲 + 章节记忆 top-k**（`build_refine_memory_context`） |
| 生成下一章 | 上项且 **含当前章**（from = 当前章号+1 ⇒ 编号 &lt; from）+ 可选当前章正文截断 |
| 刷新本章大纲 | 根参考 + 编号 &lt; 本章的大纲/记忆 + **本章链接的全部人物卡设定与全部剧情卡（含关系；按状态处理）** + 当前旧大纲（标注将被覆盖）+ 用户期望 |

`user_brief` 为空时后端按同一规则自动组装。

#### 1.0.1 根参考明细（大纲 AI 必含）

组装入口：`assemble_outline_gen_brief`。

| 块 | 函数 / 来源 | 内容 | 大纲侧约束 |
|----|-------------|------|------------|
| 简介 + 根说明 + 人物 | `root_generate_reference` | `synopsis`、根 `outline`、根链接/边关联的**人物卡** | 定调与人设 |
| **剧情卡** | `root_linked_plots_and_knowledge` | 根 `linked_side_plot_ids` + 根边上的 `side_plot`；**仅** `plot_is_injectable`（进行中且未吸收）；只注入**要点**（`outline`） | **本批须为其安排合理推进，禁止只点名** |
| **知识卡** | 同上 | 根 `linked_knowledge_ids` + 根边上的 `knowledge`；优先 `extracted`（单卡 ≤500，根合计 ≤2k），否则提取需求 / 节点 outline | **专名与规则勿与之冲突** |
| **本章链接知识卡** | `chapter_context` | 章 `linked_knowledge_ids` ∪ 章边 **+ 根/分卷继承知识卡**；写作手法/文风/用词/关键词替换等**硬约束** | **生成正文必须严格遵守** |
| **本章剧情** | `chapter_context` / `get_chapter_info` | 章本机剧情（可排序）+ 根/分卷继承剧情（**不参与本章 order**） | 根 / 卷 / 本章分别按各自 order |

说明：

- 预生成正文另有 `chapter_context`（含根贯穿卡）；**故意不**把剧情/知识并进 `root_generate_reference`，以免预生成 user 里重复两份。大纲路径单独拼 `root_linked_plots_and_knowledge`。  
- Prompt：`gen_chapter_cards_system` / `plan_next_chapters_system` 须写明「根剧情卡须推进；知识卡勿冲突」。  
- **公共知识库不是小说设定源**。写作/大纲只遵守树上挂的知识卡；Library 检索仅用于给知识卡灌内容（`fill_knowledge_card` / `search_knowledge`）。

### 1.1 根节点「生成章节卡」

| 项 | 说明 |
|----|------|
| 预览 | `preview_chapter_outline_brief(novel_id, "root", count)` |
| 命令 | `generate_chapter_cards(novel_id, count, user_brief)` |
| Prompt | `gen_chapter_cards_system` / `gen_chapter_cards_user`（user 注入编辑后的 brief） |
| 模型 | `chat_model` |
| 范围 | 第 **1–count** 章；`count` clamp 到 `1..=100`，且若 `novel.chapter_count > 0` 则不超过全书章数 |
| 冲突模式 | **Overwrite**：同号覆盖标题与大纲；无则新建并挂到章节链 |
| AI 策略 | 按总章数分配本批位置；章间衔接；不得推翻 brief 中既定事实；**推进根剧情卡**；**遵守根知识卡**；尊重用户期望 |
| UI | 根节点左侧：数量 → 确认框 → 生成；进度 3 步：context → planning → saving |

### 1.2 刷新大纲

工作台**无根 Chat / 章节卡 Chat**；斜杠指令已移除。

| 项 | 说明 |
|----|------|
| 命令 | `regenerate_chapter_outline(novel_id, node_id, user_brief, model?)` |
| 实现 | 共用 `apply_llm_chapter_cards`（与根节点「生成章节卡」同源；另附 `append_regen_outline_extras`）；结果写回触发节点 |
| Prompt | `gen_chapter_cards_system` / `gen_chapter_cards_user`（单章、`replace=true`） |
| 模型 | 传入 `model` 否则 `chat_model` |
| 范围 | **仅当前节点**；冲突模式 **Overwrite**（覆盖标题+大纲） |
| 输入材料 | 见 §1.0「刷新本章大纲」：根参考 + 前序记忆 + **本章链接的全部人物/剧情卡设定（硬约束）** + 旧大纲 + 期望 |
| AI 策略 | 覆盖重写标题与大纲；**严格遵守**本章链接人物人设与剧情卡要点（进行中推进，其余作既定事实）；固定 outline 格式；禁止另起无关主线 |
| 不改 | 正文、章节记忆、剧情卡/人物卡关联 |
| 进度 | `chat-progress` |

### 1.3 生成剧情卡

| 项 | 说明 |
|----|------|
| 命令 | `generate_chapter_plots(novel_id, node_id, user_notes, model?)` |
| 实现 | `enrich_plots_for_chapter_node` |
| Prompt | `gen_chapter_plots_system` / `gen_chapter_plots_user`（含用户补充剧情） |
| 模型 | `chat_model` |
| 产出 | 多张剧情卡挂到本章 + 补齐/关联人物卡（追加，不删已有）；新卡 `side_plot.status=active` |
| 进度 | `chat-progress` |

`plan_next_chapters` 仍保留给程序调用（生成后续章大纲）。

### 1.3c 根节点「整理剧情」（跨章导航）

| 项 | 说明 |
|----|------|
| 命令 | `consolidate_plot_cards(novel_id, chapter_window, user_notes)` |
| Prompt | `consolidate_plots_system` / `consolidate_plots_user` |
| 模型 | `chat_model` |
| 触发 | **手动**（根节点按钮）；窗口 `1..=30`，UI 默认近 5 章 |
| 输入 | 近 N 章大纲 + 章节记忆 + 现有根剧情卡 + 近章章内剧情卡 id |
| 产出 | 更新/新建**根**剧情卡（`status`: active / resolved / deferred）；可选章内卡 `absorbed=true`（保留回查，不删） |
| 注入 | 仅 **进行中且未吸收**（`plot_is_injectable`）进入 `chapter_context` / 大纲 brief / 落地检查 |
| 策略 | 跨章剧情卡＝导航；章节记忆＝刹车；整理压缩导航，不扔掉记忆 |

### 1.4 章节卡生成检查清单

改大纲 / 剧情卡生成 / 整理剧情时：

1. [ ] 更新 `preview_chapter_outline_brief` / `generate_chapter_cards` / `plan_next_chapters` / `regenerate_chapter_outline` / `generate_chapter_plots` / `consolidate_plot_cards`  
2. [ ] 更新 `assemble_outline_gen_brief` / `root_linked_plots_and_knowledge` / `plot_is_injectable`（若改材料）  
3. [ ] 更新 `outline_gen_brief` / `gen_chapter_cards_*` / `plan_next_chapters_*` / `gen_chapter_plots_*` / `consolidate_plots_*`  
4. [ ] **同步本文 §1.0 / §1.0.1 / §1.3c**（含剧情卡状态过滤）  
5. [ ] UI 文案：`messages.ts` 全语言  

---

## 2. 章节预生成（正文 Generate）

### 2.1 目的

在强约束下写出**本章完整正文**（Markdown）。篇幅以**树根节点每章目标字数**为准（见 §4）。  
**禁止参考本章已有正文/旧稿**（不读本章 md、不注入旧稿）；即使磁盘上已有正文，也按大纲与链接材料**重新生成**。改已有正文请用精修。

### 2.2 输入材料（冲突优先级）

| 材料 | 来源 | 约束角色 |
|------|------|----------|
| 全书简介 + 根节点说明 | `novel.synopsis`、根 `outline` | 定调 |
| 根节点关联人物卡 | 根链接 / 边 | 全书人设 |
| **前序大纲 + 章节记忆** | 结构树前序章 + `chapter_memory`（**可选勾选**，默认不带全历史；**排除本章**） | **既定事实，禁止推翻**（刹车） |
| **本章章纲** | 章节节点 `outline` | **主线，不可丢关键节拍** |
| **链接剧情卡**（含根贯穿） | 本章链接 + 根贯穿；**仅进行中且未吸收** | **须落地并推进**（导航） |
| 链接人物卡 + 关系边 | 本章 / 剧情卡 / 根并集 | 言行合卡 |
| 链接知识卡 | 本章 `linked_knowledge_ids` ∪ 边（及根贯穿）；`extracted` / `extract_prompt` | **写作硬约束**：手法节奏、文风语气、用词/禁用词、关键词与专名替换、视角时态、对话与修辞等 |
| 全书计划章数 | `chapter_count` | 本章信息量与悬念 |
| **每章目标字数** | §4 | **硬性篇幅（写进 Prompt）** |
| ~~本章已有正文~~ | **不注入** | 预生成禁令 |

冲突时：**前序记忆/大纲 → 本章大纲 → 剧情卡 → 人物 → 本章知识卡（硬约束文风/用词） → 简介**。未链接设定勿硬塞。公共库全文不注入写作。

### 2.3 流程

命令：章节卡工具栏「预生成」→ `generate_chapter(…, user_brief, model?)` · Prompt：`generate_chapter_*` · 模型：传入则覆盖，否则 `generate_model`  
（`preview_generate_chapter` 可供程序化勾选记忆章；工具栏默认**不带**历史记忆检索。）

1. 用户点预生成，可选附加条件（即 `user_brief`）。  
2. 组装 `root_generate_reference` +（可选）勾选**前序**章大纲/记忆 top-k（排除本章）+ `chapter_context`（含进行中剧情卡；知识卡摘要有预算）+ 用户条件；**不读**本章已有正文。  
3. `root_chapter_word_target` 解析字数；写入 system/user（目标 + ±60；**禁止越出有效区间**，见 §4）；Prompt 写明「禁止参考本章已有正文」。  
4. 代码生成「必须落地」契约（大纲节拍 / **可注入**剧情卡 / 本章焦点人物）写入 user。  
5. 首轮 LLM **重写**正文（覆盖磁盘旧稿）。  
6. **落地补写**（可选）：关键词未命中 → `repair_chapter_*` 再一轮（**不做字数验证/硬约束**，只补缺项）；Mock 跳过。  
7. 写 `chapters/{node_id}.md`（附 footer），树上字数按正文（不含 footer）。偏目标仅 `word_count_off_note`（标注，不触发重写）。  
8. **不**抽取章节记忆。  

一键自动：工具栏默认（**不带历史记忆**）+ 已存/缺省条件（靠进行中剧情卡导航）。  

进度阶段（5）：`context` → `writing` → `check_beats` →（缺项则）`repair_land` → `saving`。

### 2.4 落地检查（`chapter_constraints`）

- 大纲节拍、剧情卡要点、本章焦点人物姓名关键词命中  
- **不检查字数**（字数交由首轮预生成/精修 AI；落地轮不为字数重写全文）  
- `ponytail:` 关键词 ≠ 语义；误报高时再考虑 LLM 评审  

### 2.5 预生成检查清单

1. [ ] `generate_chapter` / `generate_chapter_*` / `repair_*` / `chapter_constraints`  
2. [ ] **同步本文 §2、§4**  
3. [ ] i18n（若 UI 变）  

---

## 3. 章节精修（Refine）

### 3.1 目的

**不大改剧情**：对照历史与本章设定，**必须在当前章已有正文底稿上**小幅梳理衔接、因果、人物、结构、语句；禁止另起炉灶重写。篇幅硬约束见 §4（目标 ±60，禁止越界）。无正文则报错，请先预生成。**不**自动抽取章节记忆。

### 3.2 输入材料

| 材料 | 说明 |
|------|------|
| **模式 A：记忆+大纲**（默认） | 前 N 章大纲 + 章节记忆；同书关键词补检索；**不注入**前序全文 |
| **模式 B：章节全文** | 前 N 章标题 + 大纲 + **完整正文** |
| 当前章链接设定 | 同预生成 `chapter_context` |
| **当前章已生成正文** | **唯一精修底稿**（必填）；必须在此原文上修改，保留骨架与段落顺序 |
| **精修条件**（可编辑） | 精修确认框；一键自动用 `localStorage` 缺省；注入 `refine_chapter_user` |
| **每章目标字数** | §4；system/user 写明具体区间 |

UI：章节卡工具栏「精修」；默认 `memory` + 前 10 章。一键自动用已存条件与面板记忆的 mode/prevN。

### 3.3a 段落改写（正文编辑）

正文面板编辑模式：左侧 gutter（约一格 icon 宽）在**非空段落首行**显示改写入口；弹层填写修改意见 → `rewrite_chapter_paragraph`（模型同全局 Chat `chat_model`）→ 回填该段并自动保存。编辑中**停手 3 秒**后自动保存（持续输入会重置计时）；「保存正文」仍可立即落盘并退出编辑。

Prompt：`rewrite_paragraph_*`。不经全局 Chat 多轮/工具链。

### 3.3 流程

命令：章节卡工具栏「精修」→ `refine_chapter(…, user_brief, model?)` · Prompt：`refine_chapter_*` · 模型：传入则覆盖，否则 `refine_model`

1. 读取当前章正文；若空则错误返回（须先预生成）。  
2. 按模式组装前 N 章材料 + `chapter_context` + **当前正文底稿** + 用户精修条件。  
3. 字数目标写入 Prompt；**无**额外篇幅校准轮次；Prompt 强调「以③原文为底稿修订，禁止丢弃另起炉灶」。  
4. 一轮 LLM 精修。  
5. 写回正文、更新树上字数。  
6. 文末修正点分类：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。偏目标仅完成消息标注。  

进度阶段（2）：`context` → `refining`。

### 3.4 策略要点

1. **底稿强制**：输出须为对当前章已有正文的修订，禁止无视原文重写（与预生成相反）。  
2. 历史只读：禁止改写历史章。  
3. 剧情卡缺漏可**小幅补写**，禁止另起炉灶。  
4. 改动克制：只改矛盾、不合理、不通顺、未落地、字数超差处。  
5. 用户精修条件须遵守，但仍不大改剧情。  
6. 建议 `refine_model` 用更强模型。  

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
| **硬约束（模型侧）** | 首轮预生成 / 精修 Prompt：正文须落在有效区间；**禁止低于**有效下限、**禁止超过**有效上限 |
| 有效区间 | `[wmin−60, wmax+60]`（`WORD_COUNT_TOLERANCE = 60`）；min=max 时为单点目标 ±60 |
| Prompt | 仅 `generate_chapter_*` / `refine_chapter_*` 写明目标与「禁止越界」；由 **AI 自行控制字数** |
| 落地 / 设定对齐 | **不做字数验证**；`repair_*` Prompt 明确禁止为凑字数重写全文 |
| 代码 | **不做**篇幅校准 LLM；越界仅完成消息 `word_count_off_note` 标注，**不**触发重写 |
| 常量 | `WORD_COUNT_TOLERANCE = 60`（`commands.rs`） |
| 适用字数硬约束 | **仅**正文预生成与精修的首轮写作 Prompt |

---

## 5. 章节记忆库

- **内容**：非完整正文。① 整体情节约 150–400 字；② 要点（人物/行为/目标/承诺/人设）。禁止照抄大段对话描写。  
- **存储**：按 `novel_id` + `node_id` → SQLite `chapter_memory` + Lance `chapter_memory`；与公共知识库分离。  
- **抽取时机**：**仅手动**（预生成 / 精修均不自动抽）。命令：`regenerate_chapter_memory(novel_id, node_id, user_notes, memory_node_ids)`；模型：`knowledge_model`。  
- **注意事项**：`localStorage`（`novework.memoryExtractNotes`）保存用户改过的条件并作为默认；首次为空时填入出厂缺省（`memoryExtractNotesDefault`）。「重置条件」写回出厂缺省。全文作为 `user_notes` 写入 `extract_chapter_memory_user`；系统 prompt 只定角色/底线。  
- **其他章记忆对照**：面板左侧勾选其他章节（默认勾选已有记忆者）；其记忆注入 prompt 仅供去重/衔接对照，**禁止**抄入本章记忆。无记忆的勾选不注入。注意事项里可写如何对照。  
- **UI**：工具栏一个「章节记忆」入口；宽面板横排「对照用其他章记忆 | 已提取记忆 | 抽取注意事项」，标题旁「重置条件」，底部提取按钮。  
- **管理**：全部章节记忆面板可删单条/清整章（`set_chapter_memory`）；删章/删书清理。  
- **检索**：写作注入在勾选范围内 `rank_memory` top-k（预算见 §1.0）。关键词为主；公共库另有本地 embeddings（MiniLM）。  
- **冷启动**：无记忆时精修仍可用大纲；需记忆时在面板手动提取。  

---

## 6. 相关 UI 与自动化

- **设置**：API / 兼容供应商与 Key；**模型在全局 Chat 发送旁选择**（写入 `chat_model`，预生成 / 精修 / 开书 / 记忆抽取共用）。**公共知识库向量索引不走 API**，使用本地 `paraphrase-multilingual-MiniLM-L12-v2`（fastembed）。  
- **根节点**：封面、每章字数、全书章数、**总纲**。画布工具栏可添加**分卷**与章节卡。MCP / 全局 Chat 仍可批量生成章节卡或整理剧情。进入工作台默认 `fitView` 完整展示当前树。**无根 Chat**。公共库不是小说设定源。  
- **分卷卡**：可选中间层（根 → 分卷 → 章，或章直接挂根）。标题与分卷纲要；可挂人物/剧情/知识（继承根只读 + 本卷可排剧情）。删除二次确认：卷下章节改挂根，不删正文。  
- **章节卡**：左侧标题、关联剧情/知识卡（**继承根 / 所属分卷**；本章剧情可拖拽排序，根/卷继承只读），**章纲**排在左栏内容最后（样式与根节点**总纲**相同）。章节正文面板工具栏：**停止**、复制、**章节记忆**、预览/编辑，位于 Markdown **预览 / 编辑**左侧（预生成/精修改由全局 Chat / MCP）。编辑模式左侧 gutter 可对单段 AI 改写（§3.3a）；编辑停手 3 秒后自动保存。画布章节卡仅显示标题 / 字数 / 章纲。MCP `get_chapter_info` **不含正文**，返回 `volume`、关联剧情（`inherited_from`: root|volume|chapter）、必出人物、关联知识卡（本章∪卷∪根），以及 `ai_guidance`（含 `content_usage`：生成勿读旧稿，精修另调 `get_chapter_content`）。MCP `get_novel_info` 返回小说简介、根上人物/剧情/知识卡与 `volumes` 摘要（不含公共库绑定）。  
- **人物卡**：左侧表单编辑全部字段 + **保存**（无人物卡 Chat）。  
- **知识卡**：勾选公共库后点**完整导入**（把所选库正文写入「已提取特征」；已有内容会先确认覆盖），或直接手写写作约束（手法/文风/用词/关键词替换等）。可选**存为公共知识卡**（跨小说目录，复制一份；无版本号）。工作台可从目录**添加公共知识卡**，挂到当前选中节点（章/分卷/根或其宿主）。Chat/MCP：`list_public_knowledge_cards` / `upsert_public_knowledge_card` / `add_public_knowledge_card`；树上卡仍用 `fill_knowledge_card` 或 `search_knowledge` 后 `upsert_knowledge_card`。MCP 新建默认挂当前选中章否则末章（`link_to` 的 `root` 会解析成根节点真实 id）；边为章.left ← 知识.right。挂到章节后，预生成/精修/**MCP 写章**须**严格遵守**。  
- **剧情卡**：标题/状态在上，**情节**排在左栏内容最后（样式与总纲/章纲相同；无剧情卡 Chat）。  
- **知识库 Library**：一行工具条：左侧 **知识源 / 知识卡**，右侧小段 **进行中 / 已归档**（两类共用）+ 导入（知识源）或**新建知识卡**（知识卡页，可空手写规则，不必先导入知识源）。**知识源**：导入 txt/epub/网址时**无 Chat、无模型选择、无 AI 分析**；元数据来自文件/网页解析；正文按 **1000 字切段、段间重叠 20 字**（epub 多章另写目录块 + 按章切段）写入 SQLite + LanceDB，并用本地 MiniLM 建索引。模型文件整文件下载到 `{data}/novework/models/paraphrase-multilingual-MiniLM-L12-v2/`（绕过 hf-hub Range/`Content-Range` 问题；可用 `HF_ENDPOINT` 指定镜像）。可查看索引状态、「重建索引」、重新入库；块列表可显示来源章名提示。**知识卡**：公共知识卡目录（手写新建，或从工作台「存为公共知识卡」写入），可编辑/归档/删除目录项（树上已挂副本不级联删除；归档后不可再挂到新小说）。  
- **画布**：添加卡片（含 MCP 新建卡）后自动一键排版；也可手动一键排版或 MCP `layout_tree`。**画布导航**（画布左侧浮动，不随缩放；工具栏 ListTree 图标切换）分 Tab：**章节**（根 + 各章）、**分卷**、**人物**、**剧情**、**知识**；点击居中放大对应卡。查看全部章节记忆。  
- **MCP**：设置可配端口（默认 `17832`）、**是否允许局域网访问**（开启则监听 `0.0.0.0`）与是否随应用启动；侧栏显示运行状态。本机 `http://127.0.0.1:{port}/mcp` 由 **`rmcp` Streamable HTTP** 提供公共知识库导入/向量检索/归档删除，公共知识卡目录（`list_public_knowledge_cards` / `upsert_public_knowledge_card` / `add_public_knowledge_card`），以及小说建改、分卷（`add_volume`）、章节大纲与正文、人物/剧情/知识卡与关联。全书快照 `get_novel_info`（含 `volumes`），写章快照 `get_chapter_info`（含分卷继承），工作台当前选中 `get_selected_card`。工作台已选中时 MCP 的 `novel_id`/`node_id` 可省略。Chat/MCP 改树后工作台会刷新。**接口字段与样例见 [`API.md`](API.md)**（改 `mcp.rs` 工具时同步）。  
- **全局 Chat**：左栏开关 + 壳层右侧**嵌入栏**（与工作台并排，不遮挡；**默认打开**，可隐藏、可拖宽，开关会记住）。进程内多轮，不跨重启持久。进入小说工作台时**绑定当前书**（栏标题为书名；对话按书隔离）；离开工作台回到全局槽。输入 `/` 或 `@` **补全 skills**（应用内置 `src-tauri/skills` + `~/.agents/skills`，同名覆盖内置）。内置 `/novel` 及其 SKILL.md 中的 MCP 工具名（如 `/novel get_selected_card`）可补全；工作台内 `/novel` 读写章/卡时可省略 `novel_id`/`node_id`（用当前选中）。**大需求**：先拆编号任务列表并直接开做（不征求「是否执行」）；需人力时用是否/单选/多选停下，选完连续做完且禁止同义再确认（任务行禁止 `- [ ]`）。「要不要继续」类是否题不展示选项芯片。助手仅在缺信息或互斥路径、不选就无法下一步时才给出是否/选项，对话里才出可点选项：单选即发，多选勾选后确认。完成操作后不追问要不要继续。发送后用户消息立即出现并显示本次 prompt token（先估算后按 API 实值更新）；等待时占位助手气泡显示当前步骤（连接 / 思考 / 调用 MCP 工具 / 写回复）与每步耗时。Chat 的 token 写入与预生成相同的 `token_usage` 表（绑定小说则计入该书；未绑定只进日/月总量）。ADK-Rust `LlmAgent` + 上述 skills；经 ADK `McpToolset`（HTTP）调用本机 MCP。须在设置中启用 MCP；**模型在发送旁选择**（OpenAI 兼容目录）。停止当前回复后会给未完成的 `tool_calls` 补一条 cancelled 结果，再发下一条才不会被 DeepSeek/OpenAI 以 400 拒绝。工作台**无根 Chat / 章节卡 Chat**。  
- **等待界面**：阶段文案 + 进度条 + 每步耗时。  

---

## 7. 总检查清单

| 改动类型 | 必须更新 |
|----------|----------|
| 章节卡 / 大纲生成 / 整理剧情 | §1（含 **§1.0.1**、**§1.3c**） |
| 预生成正文 | §2、§4 |
| 精修 / 段落改写 | §3（含 **§3.3a**）、§4 |
| 字数规则 | §4 + `WORD_COUNT_TOLERANCE` |
| 记忆抽取/存储（手动、注意事项） | §5 |
| UI 文案 | `src/i18n/messages.ts` 六语言 |

Cursor 规则：`.cursor/rules/project-md-sync.mdc`（改上述流程时强制同步本文）。
