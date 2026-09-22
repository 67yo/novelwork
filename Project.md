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
| 章节记忆 | `src-tauri/src/chapter_memory.rs` + SQLite 列表 / `rig-lancedb` 向量 |
| 知识检索预算 | `src-tauri/src/kb_context.rs`（`retrieve_knowledge` / 本地 MiniLM + `rig-sqlite`） |
| UI | `src/views/WorkspaceView.vue` |
| 设置模型 | `generate_model` / `refine_model` / `chat_model` / `knowledge_model` / `image_model` |

可取消：凡走 `arm_chat_cancel(novel_id)` 的流程均可 `chat_cancel` 中止（含落地补写、大纲生成、记忆抽取）。

进度事件：`chapter-progress`（预生成 / 精修 / 根节点生成章节卡 / 生成下一章 / 手动重抽记忆）。预生成与精修的第一步为 `confirm_model`（含实际模型名 `detail`），再进入 `context`。

---

## 0. 概念区分

| 名称 | 产出 | 典型入口 |
|------|------|----------|
| **章节卡生成** | 结构树上的章节节点：`label` + `outline`（**无正文**） | 根节点「生成章节卡」（空卡，无 AI）；大纲刷新等仍可由后端命令支持 |
| **分卷（volume）** | 可选中间层节点：根 → 分卷 → 章节；亦可章直接挂根 | 「卷与章节」Tab 添加分卷 / MCP `add_volume` |
| **预生成** | 单章 **正文** Markdown；**禁止参考本章已有正文**，按大纲/卡/前序记忆重写 | 全局 Chat「生成第 N 章」+ MCP（工作台无按钮；后端仍有 `generate_chapter`） |
| **精修** | **必须在当前已有正文上**小幅改写（无正文则拒绝；**不**抽记忆） | 全局 Chat「精修第 N 章」+ MCP（工作台无按钮；后端仍有 `refine_chapter`） |
| **章节记忆** | 摘要事实（非整章正文） | 记忆面板**手动**提取（可写注意事项） |

**分卷继承（写章 / MCP / 左栏）**

- 分卷：根设定 ∪ 分卷本机（人物 / 剧情 / 知识）。  
- 章节：根 ∪ **父分卷**（若有）∪ 本章本机。  
- 本章剧情顺序仅含本章本机；根 / 卷继承只读展示在简纲下（**+** 选已有剧情卡，本机可 **×** 解除）。卷/章人物列表默认展示根（章还含父卷）继承，只读；本机人物用 **+** 添加、芯片上 **×** 解除关联。  
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
| 知识卡 `extracted`（单卡） | 500 字；**核心法则**、**时空地理**、**社会权力**、**存在基础**与 **信息传播**（`wv_core_laws` / `wv_spatiotemporal` / `wv_social_power` / `wv_existence` / `wv_info_flow`）例外：由结构化字段 + 挂接子卡（若有）现场拼完整正文，单卡 ≤3500 |
| 根知识卡合计 | 2k；含上述结构化卡时总预算按卡数追加差额（约每张再 +3k），且核心法则优先注入 |
| 公共库检索（知识卡填充 / Chat `search_knowledge`） | 召回约 **20** ∪ 规则重排 → top **5** × **400**（带 `book_id#idx`）；评测见 `tests/fixtures/kb_retrieve_cases.json` |
| 章节记忆（勾选范围内） | top **6** × **300**，整块硬顶 **~2k**（带 `memory:node_id`） |
| 本章知识卡合计 | 2k；含上述结构化卡时同根侧追加差额 |

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
| **知识卡** | 同上 | 根 `linked_knowledge_ids` + 根边上的 `knowledge`（含世界观固定槽、**故事规则**总卡/扇卡与普通知识卡；**不含** `slot=write_prompts` 生成/精修卡及其左右子卡）；优先 `extracted`（单卡 ≤500，根合计 ≤2k），**核心法则**则现场拼立意/禁忌/力量 + **全部公理子卡**，**时空地理**则拼立意/时代等 + **全部地点子卡**，**社会权力**则拼立意/阶层/体制/可见性 + **全部种族与主要势力子卡**，**存在基础**则拼立意/死亡/历法/寿命/疫病与繁衍，**信息传播**则拼信息速度/壁垒/流言与真相/知识载体，**故事规则**则拼立意/引擎/兑现/红线（≤3500，合计预算按结构化卡数上调），否则提取需求 / 节点 outline | **专名与规则勿与之冲突** |
| **本章链接知识卡** | `chapter_context` / `get_chapter_write_context` | 知识**不向章/卷继承**。写章材料 = 根直连（含世界观+故事规则）∪ 卷直连 ∪ 章直连，**按 id 去重（先到优先）**。Chat 同一会话已提交根/卷则不再重复下发 | **生成正文必须严格遵守** |
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
| **本章简纲** | 章节节点 `outline` | **主线走向** |
| **本章细纲** | 章节节点 `detailed_outline: string[]` | **场景落地清单；正文据此扩充**（优先于简纲做验收） |
| **链接剧情卡**（含根贯穿） | 本章链接 + 根贯穿；**仅进行中且未吸收** | **须落地并推进**（导航） |
| 链接人物卡 + 关系边 | 本章 / 剧情卡 / 根并集 | 言行合卡 |
| 链接知识卡 | 本章 `linked_knowledge_ids` ∪ 边（及根贯穿）；`extracted` / `extract_prompt` | **写作硬约束**：手法节奏、文风语气、用词/禁用词、关键词与专名替换、视角时态、对话与修辞等 |
| **生成/精修卡侧挂知识** | 根上 `slot=write_prompts`：左=生成、右=精修；`write_prompt_append` 接到 `user_brief` / Chat `llm_user` 后 | **不继承、不进 `linked_knowledge`**；与用户提示同级约束 |
| 全书计划章数 | `chapter_count` | 本章信息量与悬念 |
| **每章目标字数** | §4 | **硬性篇幅（写进 Prompt）** |
| ~~本章已有正文~~ | **不注入** | 预生成禁令 |

冲突时：**前序记忆/大纲 → 本章细纲 → 本章简纲 → 剧情卡 → 人物 → 本章知识卡（硬约束文风/用词） → 简介**。未链接设定勿硬塞。公共库全文不注入写作。

### 2.3 流程

命令：`generate_chapter(…, user_brief, model?)`（后端仍保留；**工作台正文栏已去掉生成按钮**，日常写章用全局 Chat + MCP）· Prompt：`generate_chapter_*` · 模型：传入则覆盖，否则 Chat `chat_model`  
（`preview_generate_chapter` 可供程序化勾选记忆章。）

1. 用户点预生成，可选附加条件（即 `user_brief`）。  
2. **细纲**：若 `detailed_outline` 为空，则 `generate_detailed_outline`（由简纲进化并写回树）。条数按每章目标字数估算（约 320 字/场面，通常 4–10 条），避免 5–12 条一律套在短章上导致正文超字。**细纲 LLM**：简介 + 功能选项 + 章纲 + 人物（约 1200 字）+ 剧情要点（不灌剧情正文）+ **世界观/故事规则与写章同级**（结构化卡 ≤3500，普通知识卡仍 500）。JSON 解析失败则 schema 抽取 / 再修一轮（同章纲）。已有细纲则保留手改（工作台「生成细纲」走全局 Chat，MCP 侧 `force=true` 会覆盖）。左栏可**手动添加**条目、**上下排序**，或对单条点「AI 重写」（`regenerate_detailed_outline_item`，先填可选提示）；细纲标题行：**生成细纲** | **添加** | **生成章节** | **精修**（后三项中后两个经全局 Chat 发「生成第 N 章正文 / 精修第 N 章正文」）。  
3. 组装 `root_generate_reference` +（可选）勾选**前序**章大纲/记忆 top-k（排除本章）+ `chapter_context`（含简纲+细纲与进行中剧情卡；知识卡摘要有预算）+ 用户条件；**不读**本章已有正文。  
4. `root_chapter_word_target` 解析字数；写入 system/user（目标 + ±60）。Prompt：瞄准中位一次交卷，按细纲条数均分篇幅；偏长只删冗、**禁止整章重写**（见 §4）。  
5. 代码生成「必须落地」契约（**优先细纲分条**，否则简纲节拍 / **可注入**剧情卡 / 本章焦点人物）写入 user。  
6. 首轮 LLM **重写**正文（覆盖磁盘旧稿）。  
7. **落地补写**（可选）：关键词未命中 → `repair_chapter_*` 再一轮（**不做字数验证/硬约束**，只补缺项）；Mock 跳过。  
8. **设定校对**：对照根上世界观六卡 + 故事规则（`verify_lore_chapter_*`）。无冲突输出 `LORE_OK` 保留正文；有冲突就地修订（骨架不变，keep-guard 同落地轮）。无设定或 Mock 则跳过。  
9. 写 `chapters/{node_id}.md`（附 footer），树上字数按正文（不含 footer）。偏目标仅 `word_count_off_note`（标注，不触发重写）。  
10. **不**抽取章节记忆。  

一键自动：工具栏默认（**不带历史记忆**）+ 已存/缺省条件（靠进行中剧情卡导航）。  

进度阶段：`confirm_model` → `context` → `detailed_outline` → `writing` → `check_beats` →（缺项则）`repair_land` → `check_lore` → `saving`。

### 2.4 落地检查（`chapter_constraints`）

- 大纲节拍、剧情卡要点、本章焦点人物姓名关键词命中  
- **不检查字数**（字数交由首轮预生成/精修 AI；落地轮不为字数重写全文）  
- `ponytail:` 关键词 ≠ 语义；误报高时再考虑 LLM 评审  

### 2.4a 设定校对（世界观 / 故事规则）

- 材料：根上六张世界观卡 + 故事规则总卡（与写作注入同形 `knowledge_card_inject_body`；总长封顶）  
- 一轮 LLM：无冲突则 `LORE_OK`；有冲突则就地修订，**不做字数验证**  
- 精修共用；Chat 路径靠 `ai_guidance.lore`（不在 `set_chapter_content` 上强制跑 LLM）  
- 无设定或 Mock 跳过  

### 2.5 预生成检查清单

1. [ ] `generate_chapter` / `generate_chapter_*` / `repair_*` / `verify_lore_chapter_*` / `chapter_constraints`  
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
| **精修侧挂知识** | 根「生成/精修」卡右侧知识；接到用户精修条件后；不进章节继承 |
| **每章目标字数** | §4；system/user 写明具体区间 |

UI：精修参考默认 `memory` + 前 10 章（后端参数；Chat 路径由 Agent 自行取上下文）。

### 3.3a 段落改写（正文编辑）

正文面板**默认即为全文编辑**（无 Markdown 预览切换），使用 **CodeMirror 6 稿纸**（米色信纸 + 横线 + 红栏）。**最左列为 CodeMirror gutter**：鼠标移入某非空段落时显示改写入口（与该段对齐并随滚动）；弹层填写修改意见 → `rewrite_chapter_paragraph`（模型同全局 Chat `chat_model`）→ 回填该段并自动保存。回填后**选中被改写的段、光标在段末**，滚动条位置与改写前相同。输入人物名时弹出树上人物专名补全（Tab / Enter）。编辑中**停手 3 秒**后自动保存（持续输入会重置计时；无单独「保存正文」按钮）。全局 Chat 经 MCP `set_chapter_content` 生成/精修落盘后，工作台切到该章并刷新正文；若有改动，正文面板可在「正文 / 更新对比」间切换（红＝更新前，绿＝更新后）。对比以点击生成/精修时的正文为「更新前」（多次 set 合并，避免第二次写入把对比冲掉）。连续改动收成段对比，段头可选保留更新前或更新后；选定后该段对比消失，正文留下所选内容并保存。

Prompt：`rewrite_paragraph_*`。不经全局 Chat 多轮/工具链。

### 3.3 流程

命令：`refine_chapter(…, user_brief, model?)`（后端仍保留；**工作台正文栏已去掉精修按钮**，日常精修用全局 Chat「精修第 N 章」+ MCP）· Prompt：`refine_chapter_*` · 模型：传入则覆盖，否则 Chat `chat_model`

1. 读取当前章正文；若空则错误返回（须先预生成）。  
2. 按模式组装前 N 章材料 + `chapter_context` + **当前正文底稿** + 用户精修条件。  
3. 字数目标写入 Prompt；**无**额外篇幅校准轮次；Prompt 强调「以③原文为底稿修订，禁止丢弃另起炉灶」。  
4. 一轮 LLM 精修。  
5. **设定校对**：同 §2.4a（世界观/故事规则）。  
6. 写回正文、更新树上字数。  
7. 文末修正点分类：历史衔接 / 剧情卡落地 / 情节错误 / 人物关系 / 结构 / 语句 / 字数。偏目标仅完成消息标注。  

进度阶段：`confirm_model` → `context` → `refining` → `check_lore` → `saving`。

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

1. [ ] `refine_chapter` / `refine_chapter_*` / `verify_lore_chapter_*` / 记忆抽取  
2. [ ] **同步本文 §3、§4、§5**  
3. [ ] i18n（若 UI 变）  

---

## 4. 硬性篇幅（预生成 / 精修共用）

| 项 | 规则 |
|----|------|
| 目标来源 | 树根 `word_count_min` / `word_count_max` → 否则 `novel` 同字段 → 默认 2000–3000 |
| 计数 | 非空白字符（`count_words`） |
| **硬约束（模型侧）** | 首轮瞄准目标中位、按细纲条数均分；±60 为标注带。Chat：`set_chapter_content` 返回 `in_band` 后禁止再为篇幅重写 |
| 有效区间 | `[wmin−60, wmax+60]`（`WORD_COUNT_TOLERANCE = 60`）；min=max 时为单点目标 ±60 |
| Prompt | `generate_detailed_outline_*` 写入条数预算；`generate_chapter_*` / `refine_chapter_*` / Chat `ai_guidance.length` 写明一次交卷、禁止整章重写 |
| 落地 / 设定对齐 | **不做字数验证**；`repair_*` / `verify_lore_chapter_*` Prompt 明确禁止为凑字数重写全文 |
| 代码 | **不做**篇幅校准 LLM；越界仅完成消息 `word_count_off_note` 标注，**不**触发重写。Chat 靠 `in_band` 停手 |
| 常量 | `WORD_COUNT_TOLERANCE = 60`；细纲节奏 `detailed_outline_pace`（约 320 字/条，4–10 条） |
| 适用字数硬约束 | **仅**正文预生成与精修的首轮写作 Prompt |

---

## 5. 章节记忆库

- **内容**：非完整正文。① 整体情节约 150–400 字；② 要点（人物/行为/目标/承诺/人设）；③ **已交代**（本章已向读者说明的观点/设定/判断，供后文禁止再解说；对照章已有项不抄入）。禁止照抄大段对话描写。  
- **存储**：按 `novel_id` + `node_id` → SQLite `chapter_memory` 列表 + `rig-lancedb` 向量表 `chapter_memory_vec`；与公共知识库分离。  
- **抽取时机**：**仅手动**（预生成 / 精修均不自动抽）。命令：`regenerate_chapter_memory(novel_id, node_id, user_notes, memory_node_ids)`；模型：`knowledge_model`。抽取走 rig Extractor 结构化（`plot` / `facts` / `revealed`）；失败或空则回退原 `llm_complete` + 文本切分。模拟模式不写库。  
- **注意事项**：`localStorage`（`novework.memoryExtractNotes`）保存用户改过的条件并作为默认；首次为空时填入出厂缺省（`memoryExtractNotesDefault`）。「重置条件」写回出厂缺省。全文作为 `user_notes` 写入 `extract_chapter_memory_user`；系统 prompt 只定角色/底线。  
- **其他章记忆对照**：面板左侧勾选其他章节（默认勾选已有记忆者）；其记忆注入 prompt 仅供去重/衔接对照，**禁止**抄入本章记忆。无记忆的勾选不注入。注意事项里可写如何对照。  
- **UI**：工具栏一个「章节记忆」入口；宽面板横排「对照用其他章记忆 | 已提取记忆 | 抽取注意事项」，标题旁「重置条件」，底部提取按钮。  
- **管理**：全部章节记忆面板可删单条/清整章（`set_chapter_memory`）；删章/删书清理。  
- **检索**：写作注入（`build_refine_memory_context`）与 Chat RAG 均在范围内 **MiniLM 向量优先**（`rig-lancedb`），无命中再 `rank_memory` 关键词；预算见 §1.0。公共库另有 `rig-sqlite`（sqlite-vec）+ MiniLM。  
- **冷启动**：无记忆时精修仍可用大纲；需记忆时在面板手动提取。  

---

## 6. 相关 UI 与自动化

- **左栏导航**：「小说」回到上次打开的工作台并保持上次选中的章/卡；「列表」退出到小说目录。  
- **设置**：**界面主题**（浅色 / 深色 / 跟随系统）；**稿纸配色**（浅：白 / 米色 / 浅绿 / 浅青；深：夜黑 / 暖灰 / 夜绿 / 夜蓝，与界面主题独立）；**AI API**（协议 + Key；Gemini / Claude 走协议下拉，无独立 Key 栏）；**模型在全局 Chat 发送旁选择**（写入 `chat_model`，预生成 / 精修 / 开书 / 记忆抽取共用）；设置里另选 **文生图模型**（`image_model`，封面/人物设定图提示词；空则跟 Chat）。**公共知识库向量索引不走 API**，使用本地 `paraphrase-multilingual-MiniLM-L12-v2`（fastembed）。  
- **工作台七 Tab**（结构树数据 / 边 / `linked_*` / MCP 读写不变）：**全书**（左侧窄导航：封面与规划、**生成/精修**按生成/精修用知识分组、根上普通知识；右侧编辑封面、每章字数、全书章数、**功能选项**、**总纲**与所选知识卡；选中生成/精修后右侧两块（生成用 / 精修用），各 **+** 从公共库复制挂到该侧，本机可 **×**；可从公共库导入普通知识挂到根；**无根 Chat**）、**世界观**（同样左侧窄列表点进既有 Panel / 子卡；**生成世界观** Chat 在此 Tab）、**人物**（左侧窄导航按全书 / 卷 / 章分组列出人物卡，右侧结构化表单）、**故事规则**（左侧窄导航：总卡 + 表层设定 / 故事引擎 / 兑现系统 / 约束红线；右侧编辑；hub Chat）、**卷与章节**（左侧卷→章列表；工具条 **统计字数** icon：全部或第 X–Y 章，读正文去空白、无 AI；右侧编辑章/卷；**点章**才显示正文 dock，可停靠编辑栏右侧或下方；点卷则隐藏正文，导航仍在左、卷编辑在右）、**自定义剧情**（左侧窄导航按全书 / 卷 / 章分组列出剧情卡，天蓝色；右侧编辑标题/状态/情节；本 Tab **添加剧情卡**挂到根）、**结构树**（Rete.js 整棵树画布：点选、拖动落盘坐标、拖线关联；双击连线删除；人物↔人物线可标关系；左上角**添加卡片**（章/卷/人物/剧情/知识/公共知识）、**画布导航**（章/卷/人物/剧情/知识，点条目定位）与**一键排版**。左侧仍为当前卡编辑栏）。公共库不是小说设定源。MCP / 全局 Chat 仍可批量生成章节卡或整理剧情。MCP `layout_tree` 仍可整列节点坐标。  
- **分卷卡**：可选中间层（根 → 分卷 → 章，或章直接挂根）。标题与分卷纲要；人物列表默认带全书继承（只读，不可改根上人物），后 **+** 从尚未挂到本卷（且非根继承）的人物中选择并建立关联，本机人名可 **×** 解除（不删人物卡）。知识卡不在卷表单编辑（写章仍按根/卷/章并集取知识）。剧情卡在「自定义剧情」Tab 编辑，数据仍可挂根/卷/章。删除二次确认：卷下章节改挂根，不删正文。  
- **章节卡**：在「卷与章节」Tab 点章后，右侧标题、人物列表（默认带全书与所属分卷继承，只读，不可改卷/根上人物；后 **+** 关联尚未挂到本章的人物，本机人名可 **×** 解除；写章由 `get_chapter_write_context` 取根+卷+章并按 id 去重），**简纲** + **关联剧情卡**（根/卷继承只读，**+** 选已有剧情卡，本机可 **×**）+ **细纲列表**排在编辑栏内容最后（细纲标题行：**生成细纲**（全局 Chat「生成本章细纲」）| **添加** | **生成章节**（「生成第 N 章正文」）| **精修**（「精修第 N 章正文」）；条目可**上下排序**、**单条 AI 重写**（先填提示）或手改，失焦自动保存）。章节正文面板**仅在该 Tab**、**默认全屏稿纸编辑**（CodeMirror 6，无预览模式）；工具栏：**停止**（其它 AI 任务进行中）、**查找**（CodeMirror 搜索面板：下一个 / 上一个，面板内可替换）、**朗读**（kokoro-tts 读当前正文；首次下载模型时弹出进度窗；播放中选中当前句并滚入视野；工具栏可选 1×/2×/3× 倍速）、复制、**分镜头**（按正文拆镜 → 生成 MiniMax/ComfyUI 提示词 → 提交本机 ComfyUI Desktop 队列；覆盖已有镜头二次确认）、**章节记忆**、停靠位置（右/下）。**生成/精修正文请用全局 Chat**（「生成第 N 章」「精修第 N 章」+ MCP）；点生成/精修前会先把编辑器正文（含删空）写入磁盘，再发 Chat。`set_chapter_content` 后工作台切到该章刷新正文，有改动则显示「更新对比」（段头可选保留更新前或更新后）。CodeMirror 最左列 gutter **悬停段落时**显示 AI 改写入口（§3.3a）；停手 3 秒自动保存（无单独「保存正文」按钮）。MCP `get_chapter_info` **不含正文**，知识仅为本章直连；写章用 `get_chapter_write_context`（根/卷可按会话省略）。MCP `get_novel_info` 返回小说简介、根上人物/剧情/知识卡与 `volumes` 摘要（不含公共库绑定）。  
- **人物卡**：「人物」Tab 左侧窄列表（按挂载：全书 / 卷 / 章；琥珀色左边线）；点卡后右侧结构化表单（真名/别称/世界位置/锚点/关系网/信念/深层/声线含**肢体语言**）+ **保存** + **AI 改写整卡**（须填满全部字段）。卷/章编辑栏人物列表默认展示根（章还含父卷）继承只读，后可 **+** 关联未挂到本卡的人物，本机人名可 **×** 解除关联。MCP：`get_character_card`（完整 `character` + `formatted`）/ `upsert_character_card`（可传完整 `character` 对象）。写章注入人设全文。  
- **知识卡**：勾选公共库后点**完整导入**（把所选库正文写入「已提取特征」；已有内容会先确认覆盖），或直接手写写作约束（手法/文风/用词/关键词替换等）。可选**存为公共知识卡**（跨小说目录，复制一份；无版本号）。**全书** Tab 可从目录**添加公共知识卡**（挂到根）；生成/精修编辑里 **+** 的副本挂到该总卡左/右侧，不挂根。根上固定槽在 UI 上拆开：世界观六卡在「世界观」Tab，故事规则四块在「故事规则」Tab，生成/精修与普通知识在「全书」Tab。六张世界观卡均可 **Chat 补全本卡**（对话生成该卡全部字段与子项，不覆盖其它卡）。**核心法则**（`slot=wv_core_laws`）左侧：一句话立意、**世界公理标题列表**（点标题打开子卡；**添加公理**在核心法则上方扇形挂 `wv_axiom` 知识卡，可删）、禁忌红线、力量体系、力量表现。公理字段（名称/表述/边界/代价/执行机制）在子卡编辑（可单字段或 **AI 重写整条**），汇总进核心法则 `extracted`；写作注入时核心法则正文含完整公理子卡。旧内嵌 `axioms` 打开树时自动迁为子卡。**时空地理**（`slot=wv_spatiotemporal`）：一句话立意、时代背景、生态、世界格局、**关键地点标题列表**（点标题打开子卡；**添加地点**在时空地理上方扇形挂 `wv_location` 知识卡，可删；可 **AI 一次性生成**多条子卡）、环境质感；地点字段（名称/特征/地貌/控制势力）在子卡编辑（可单字段或 **AI 重写整条**），汇总进时空地理 `extracted`；写作注入含完整地点子卡。旧内嵌 `locations` 打开树时自动迁为子卡。**社会权力**（`slot=wv_social_power`）：一句话立意、**种族标题列表**（点标题打开子卡；**添加种族**在社会权力上方扇形左弧挂 `wv_race` 知识卡，可删；画布玫瑰色）、**主要势力标题列表**（点标题打开子卡；**添加势力**在右弧挂 `wv_faction` 知识卡，可删；画布靛蓝色）、阶层结构（一句话）、政治体制、权力可见性。种族字段（名称/特征/人口/社会地位）与势力字段（类型/目标/手段/权力基础）在子卡编辑（可单字段或 **AI 重写整条**），汇总进社会权力 `extracted`；写作注入含完整种族与势力子卡。旧内嵌 `races`/`factions` 打开树时自动迁为子卡。**存在基础**（`slot=wv_existence`）：一句话立意、死亡、历法、寿命、疫病与繁衍；各字段可 **AI 改写**，保存后同步 `extracted`；写作注入由 `existence_fmt` 拼完整结构化正文。**信息传播**（`slot=wv_info_flow`）：一句话立意、信息速度、信息壁垒、流言与真相、知识载体；各字段可 **AI 改写**，保存后同步 `extracted`；写作注入由 `info_flow_fmt` 拼完整结构化正文。Chat/MCP：`list_public_knowledge_cards` / `upsert_public_knowledge_card` / `add_public_knowledge_card`；树上卡仍用 `fill_knowledge_card` 或 `search_knowledge` 后 `upsert_knowledge_card`。MCP 新建默认挂当前选中章否则末章（`link_to` 的 `root` 会解析成根节点真实 id）；边为章.left ← 知识.right（根上固定槽：世界大纲 `wv`→bottom，故事规则 `sr`→left，生成/精修 `wp`→right）。挂到章节后，预生成/精修/**MCP 写章**须**严格遵守**。  
- **剧情卡**：在「自定义剧情」Tab 左侧按挂载（全书 / 卷 / 章）列出；点卡后右侧标题/状态 + **情节**（样式与总纲/章纲相同；无剧情卡 Chat）。本 Tab 添加的新卡挂到根。查看全部章节记忆入口仍在「卷与章节」Tab。  
- **知识库 Library**：一行工具条：左侧 **知识源 / 知识卡**，右侧小段 **进行中 / 已归档**（两类共用）+ 导入（知识源）或**新建知识卡**（知识卡页，可空手写规则，不必先导入知识源）。**知识源**：导入 txt/epub/网址时**无 Chat、无模型选择、无 AI 分析**；元数据来自文件/网页解析；正文按 **1000 字切段、段间重叠 20 字**（epub 多章另写目录块 + 按章切段）写入 SQLite，并用本地 MiniLM 经 `rig-sqlite`（sqlite-vec）建索引。模型文件整文件下载到 `{data}/novework/models/paraphrase-multilingual-MiniLM-L12-v2/`（绕过 hf-hub Range/`Content-Range` 问题；可用 `HF_ENDPOINT` 指定镜像）。可查看索引状态、「重建索引」、重新入库；块列表可显示来源章名提示。**知识卡**：公共知识卡目录（手写新建，或从工作台「存为公共知识卡」写入），可编辑/归档/删除目录项（树上已挂副本不级联删除；归档后不可再挂到新小说）。  
- **MCP**：设置可配端口（默认 `17832`）、**是否允许局域网访问**（开启则监听 `0.0.0.0`）与是否随应用启动；侧栏显示运行状态。本机 `http://127.0.0.1:{port}/mcp` 由 **`rmcp` Streamable HTTP** 提供公共知识库导入/向量检索/归档删除，公共知识卡目录（`list_public_knowledge_cards` / `upsert_public_knowledge_card` / `add_public_knowledge_card`），以及小说建改（含 `features` 功能选项）、分卷（`add_volume`）、章节大纲与正文、**分镜头**（`get_chapter_shots` / `set_chapter_shots`；`split_chapter_shots` / `generate_shot_comfy_prompts` **只返回材料，不调应用 LLM**，由客户端写完再 `set_chapter_shots`；`submit_chapter_shots_comfyui`）、**章节记忆**（`get_chapter_memory` / `list_chapter_memory` / `set_chapter_memory` / `regenerate_chapter_memory`）、**世界观**（`get_worldview` / `ensure_worldview` / `apply_worldview` / `generate_worldview`，生成须服从功能选项）、人物（`get_character_card` / `upsert_character_card`）、剧情/知识卡与关联（加卡工具支持 `items[]` 批量）。全书快照 `get_novel_info`（含 `volumes` 与 `features`），写章 `get_chapter_write_context`（根/卷按会话去重；知识不向章继承），章快照 `get_chapter_info`，工作台当前选中 `get_selected_card`。工作台已选中时 MCP 的 `novel_id`/`node_id` 可省略；`node_id` 也可传「第N章」/章号/唯一标题。Chat/MCP 改树后工作台会刷新。**接口字段与样例见 [`API.md`](API.md)**；**载荷格式变更须同步 MCP**（见 `.cursor/rules/mcp-sync.mdc`）。  
- **全局 Chat**：右下角可拖动圆形入口打开（打开后入口隐藏，关闭 Chat 后出现）；**可停靠右侧或浮窗**（浮窗可拖动、右下角改大小；位置与模式会记住）。进程内多轮，不跨重启持久。`ConversationMemory` 经 `rig-memory`：`TokenWindowMemory`（约 8k token 启发式预算）+ `CompactingMemory`/`TemplateCompactor`，超出窗口的轮次收成一条「【更早轮次】」；load/append 仍 stub 成功写入的正文/整卡 JSON。非法 tool_call 由 hook 反馈并最多重试 2 次；流式拒绝轮次清空已推送正文。绑定小说且非写章意图时 `dynamic_context` 按用户话检索该书**章节记忆**（不是公共库）。进入小说工作台时**绑定当前书**（栏标题为书名；对话按书隔离）；离开工作台回到全局槽。输入 `/` 或 `@` **补全 skills**（应用内置 `src-tauri/skills` + `~/.novelwork`，同名覆盖内置）。**仅显式** `/name` 或 `@name` 才把 SKILL.md 注入本轮（无自动匹配）。应用内 Chat **按意图裁剪 MCP 工具**（写章只留 `get_chapter_write_context` / `generate_detailed_outline` / `get_chapter_content` / `set_chapter_content`；改卡只留 `get_selected_card` / `get_character_card` / `upsert_*` / `fill_knowledge_card` / `link_nodes` / `unlink_nodes`；细纲只留 `get_selected_card` / `generate_detailed_outline` / `regenerate_detailed_outline_item` / `update_chapter_outline`；分镜、公共库、世界观生成默认不暴露；`/novel` 才给全套）。system + tools 为稳定前缀；仅 OpenAI Completions 协议带 `prompt_cache_key`（按意图+小说）。Chat 按设置 **AI API** 协议选 rig 客户端（`openai` Completions、`openai_responses`、`deepseek`、`zai`、`anthropic`、`gemini`、`groq`、`moonshot`、`mistral`、`openrouter`、`together`、`xai`、`ollama`、`hyperbolic`、`huggingface`、`minimax`、`mira`、`perplexity`、`venice`、`cohere`、`azure`、`llamafile`、`xiaomimimo`、`doubleword`；阿里云无独立库，走 Completions）。内置 `/novel` 及其 SKILL.md 中的 MCP 工具名（如 `/novel get_selected_card`）可补全；工作台内 `/novel` 读写章/卡时可省略 `novel_id`/`node_id`（用当前选中）。**「生成/重写第 N 章正文」**：系统指令只要求 `get_chapter_write_context` 并遵守返回的 `ai_guidance`（已有根则 `include_root=false`，同卷已有则 `include_volume=false`；**不要新开 session**）；不必先打 `/novel`。新生成禁止读旧稿，重写须 `get_chapter_content`；落盘前自检再 `set_chapter_content`（软约束，含 `ai_guidance.lore` 世界观/故事规则；非工作台 `generate_chapter` 流水线）。**大需求**：先拆编号任务列表并直接开做（不征求「是否执行」）；任务行禁止 `- [ ]`。缺信息或互斥路径时进程内工具 `ask_user` 暂停本轮并弹出选项芯片（**不是 MCP**；点选经 `global_chat_answer_ask` 作为 tool result 继续，禁止只在正文列 1. 2. 3.）。「要不要继续」类不问。正文编号选项仍作 fallback 芯片。单选即答，多选勾选后确认。完成操作后不追问要不要继续。发送后用户消息立即出现并显示本次 prompt token（先估算后按 API 实值更新）；等待时占位助手气泡显示当前步骤（连接 / 思考 / 调用 MCP 工具 / 写回复）与每步耗时。Chat 的 token 写入与预生成相同的 `token_usage` 表（绑定小说则计入该书；未绑定只进日/月总量）。`rig-agent` + 上述 skills；经本机 MCP HTTP 把工具包成 `PortableDynamicTool` 再调 `rmcp`。须在设置中启用 MCP；**模型在发送旁选择**（AI API 目录）。停止当前回复后本轮不写入历史（并撤回刚发出的用户气泡），下一条从停止前的历史继续。工作台**无根 Chat / 章节卡 Chat**。  
- **等待界面**：阶段文案 + 进度条 + 每步耗时。  

---

## 7. 总检查清单

| 改动类型 | 必须更新 |
|----------|----------|
| 树上载荷 / 写作注入格式 / MCP 工具 | `mcp.rs` + `API.md` + `skills/novel/SKILL.md`（见 `.cursor/rules/mcp-sync.mdc`） |
| 章节卡 / 大纲生成 / 整理剧情 | §1（含 **§1.0.1**、**§1.3c**） |
| 预生成正文 | §2、§4 |
| 精修 / 段落改写 | §3（含 **§3.3a**）、§4 |
| 字数规则 | §4 + `WORD_COUNT_TOLERANCE` |
| 记忆抽取/存储（手动、注意事项） | §5 |
| UI 文案 | `src/i18n/messages.ts` 六语言 |

Cursor 规则：`.cursor/rules/project-md-sync.mdc`（章节 AI 流程）、`.cursor/rules/mcp-sync.mdc`（载荷与 MCP 契约）、`.cursor/rules/i18n-sync.mdc`（界面文案）。
