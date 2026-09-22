# Novel Work MCP API

本文记录 **本机 MCP 服务**的请求与返回约定。实现：`rmcp` Streamable HTTP（`src-tauri/src/mcp.rs`）。改工具参数或返回结构时请同步更新本文。

章节写作策略见 `Project.md`；本文只覆盖 MCP 对外接口。

---

## 1. 连接

| 项 | 说明 |
|----|------|
| 默认地址 | `http://127.0.0.1:17832/mcp` |
| 端口 | 设置 → MCP 服务（默认 `17832`，范围 1024–65535） |
| 实现 | **`rmcp` ≥ 3.1** Streamable HTTP（含环回 Host 校验） |
| 协议 | MCP Streamable HTTP；客户端按 MCP 规范 `initialize` / `tools/list` / `tools/call` |
| 绑定 | 默认 `127.0.0.1`；设置中开启 **允许局域网访问** 后监听 `0.0.0.0`（局域网设备用 `http://<本机IP>:{port}/mcp`） |
| 响应 | 默认偏好 `application/json`；若中间有 notification 则回落 SSE |

Cursor / 客户端示例：

```json
{
  "mcpServers": {
    "nove-work": {
      "url": "http://127.0.0.1:17832/mcp"
    }
  }
}
```

兼容说明：Cursor / 其它标准 Streamable HTTP 客户端即可；勿再假定「纯手写 JSON-RPC + GET 说明页」。

---

## 2. 传输层：JSON-RPC（MCP 消息）

### 2.1 通用请求

```http
POST /mcp HTTP/1.1
Host: 127.0.0.1:17832
Content-Type: application/json
Accept: application/json, text/event-stream
```

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "list_knowledge",
    "arguments": { "include_archived": false }
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `jsonrpc` | string | 固定 `"2.0"` |
| `id` | number \| string \| null | 请求 id；通知可省略或为 `null` |
| `method` | string | 见下表 |
| `params` | object | 方法参数 |

### 2.2 生命周期方法

| method | 说明 |
|--------|------|
| `initialize` | 握手，返回协议与 serverInfo |
| `notifications/initialized` | 客户端通知；服务端 `202`，无 body |
| `ping` | 心跳，返回 `{}` |
| `tools/list` | 列出全部工具与 inputSchema |
| `tools/call` | 调用工具 |

#### `initialize` 返回样本

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-03-26",
    "capabilities": { "tools": {} },
    "serverInfo": { "name": "nove-work", "version": "0.1.0" }
  }
}
```

#### `tools/call` 成功外壳

工具业务结果放在 `content[0].text` 里，一般为 **JSON 字符串**（再 `JSON.parse` 一次）。

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "[{\"id\":\"...\",\"title\":\"...\"}]"
      }
    ],
    "isError": false
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `result.content` | array | MCP content blocks |
| `result.content[].type` | string | 固定 `"text"` |
| `result.content[].text` | string | 业务 JSON 或错误文案 |
| `result.isError` | boolean | 工具业务失败时为 `true`（仍走 result，不是 JSON-RPC error） |

#### 错误样本（工具业务失败）

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [{ "type": "text", "text": "请先归档后再删除" }],
    "isError": true
  }
}
```

#### 错误样本（未知 method）

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "method not found: foo"
  }
}
```

下文「请求」指 `tools/call` 的 `params.arguments`；「返回」指 `content[0].text` 解析后的 JSON（`get_chapter_content` 除外，其为 Markdown 原文）。

---

## 3. 公共类型

### 3.1 KnowledgeBook（公共知识库）

```json
{
  "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "title": "仙侠世界观设定集",
  "author": "未知作者",
  "genres": ["玄幻", "设定"],
  "source_path": "/Users/me/books/world.epub",
  "extract_prompt": "",
  "created_at": "2026-08-11T12:00:00+00:00",
  "chunk_count": 42,
  "archived": false
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | UUID |
| `title` | string | 书名（导入时可覆盖；EPUB 可从元数据识别） |
| `author` | string | 作者 |
| `genres` | string[] | 标签/题材 |
| `source_path` | string | 本地路径或导入来源 |
| `extract_prompt` | string | 遗留字段；公共库导入通常为空 |
| `created_at` | string | RFC3339 |
| `chunk_count` | number | 切段数量 |
| `archived` | boolean | 是否已归档 |

### 3.2 NovelProject（小说）

```json
{
  "id": "n-uuid",
  "title": "夜航船",
  "synopsis": "一个关于港口与记忆的故事。",
  "cover_path": null,
  "knowledge_ids": [],
  "knowledge_strategy": "",
  "canon_mode": "reference",
  "archived": false,
  "word_count_min": 2000,
  "word_count_max": 3000,
  "chapter_count": 20,
  "features": {
    "genres": ["urban"],
    "core_play": ["system"],
    "styles": ["cool"],
    "relationships": ["single_heroine"],
    "audiences": ["male"]
  },
  "created_at": "2026-08-11T12:00:00+00:00",
  "updated_at": "2026-08-11T12:00:00+00:00"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 小说 UUID |
| `title` | string | 标题 |
| `synopsis` | string | 简介 |
| `cover_path` | string \| null | 封面路径 |
| `knowledge_ids` | string[] | 遗留字段；**不是**小说设定源，写作不注入 |
| `knowledge_strategy` | string | 遗留字段；写作不注入 |
| `canon_mode` | string | 遗留字段（`reference` / `strict`）；写作不注入 |
| `archived` | boolean | 是否归档 |
| `word_count_min` / `word_count_max` | number | 每章目标字数区间 |
| `chapter_count` | number | 全书计划章数（1–500） |
| `features` | object | 功能选项：题材 / 核心玩法 / 风格 / 关系 / 受众（均为 string[]；预设 id + 可选自定义文本） |
| `created_at` / `updated_at` | string | RFC3339 |

### 3.3 NovelTree / TreeNode / TreeEdge

```json
{
  "novel_id": "n-uuid",
  "nodes": [
    {
      "id": "root",
      "kind": "novel",
      "label": "夜航船",
      "outline": "简介…",
      "character": null,
      "knowledge": null,
      "side_plot": null,
      "linked_character_ids": [],
      "linked_side_plot_ids": [],
      "linked_knowledge_ids": [],
      "position": { "x": 280.0, "y": 0.0 },
      "word_count": 0,
      "word_count_min": 2000,
      "word_count_max": 3000,
      "chapter_count": 20
    }
  ],
  "edges": [
    {
      "id": "e-root-ch-1",
      "source": "root",
      "target": "ch-uuid",
      "kind": "chapter",
      "source_handle": "bottom",
      "target_handle": "top",
      "label": ""
    }
  ]
}
```

**`kind`（节点）**：`novel` \| `volume` \| `chapter` \| `character` \| `side_plot` \| `knowledge`

**`kind`（边）**：`chapter` \| `volume` \| `character` \| `side_plot` \| `knowledge` 等

**`source_handle` / `target_handle`**：卷/章卡 `top`/`bottom`（脊柱）、`left`（人物/知识）、`right`（剧情）。根节点固定槽另用 `wv`（世界大纲扇形）、`sr`（故事规则）、`wp`（生成/精修枢纽）。

| TreeNode 字段 | 说明 |
|---------------|------|
| `label` | 显示名 / 章标题 |
| `outline` | 简纲（章节）/ 大纲或简介文本 |
| `detailed_outline` | 细纲分条（`string[]`，章节；缺省 `[]`） |
| `volume` | 分卷载荷 `VolumePayload`（仅 `volume`；**不写** `outline`） |
| `character` | 人物卡载荷（仅 `character`） |
| `knowledge` | 树上知识卡载荷（仅 `knowledge`） |
| `side_plot` | 剧情卡元数据（仅 `side_plot`） |
| `linked_*_ids` | 挂在本章/根/分卷上的卡 id |
| `word_count` | 本章已写正文非空白字数 |
| `word_count_min/max`、`chapter_count` | 根节点计划字段 |

**CharacterCard**（`get_character_card` / `linked_characters[].character` 完整序列化）

| 字段 | 说明 |
|------|------|
| `role` / `gender` / `age` / `alignment` / `style` / `motto` | 基础信息（扁平；可与结构化并存）。MCP **不读写** `personality`（遗留画布摘要，由深层字段生成） |
| `aliases` | 别称 |
| `constraints` | 写该人物时的硬约束长文本（常与 `formatted` 同步） |
| `world_position` | 出身阶层 / 势力 / 社会角色 / 冲突立场 |
| `world_anchors` | 世界锚点：`embodies_law`/`embodies_note`、`shaped_by_law`/`shaped_by_note`、`will_challenge` |
| `relations[]` | 关系网：`name`/`relation`/`definition`/`default_attitude`/`hidden_tension` |
| `core_belief` | 核心信念：`belief`/`author_verdict`/`source` |
| `deep` | 深层弧光（创伤、矛盾、成长等） |
| `voice` | 角色声线：`positioning`/`cognitive_filter`/`body_language`（肢体语言）、`sentence_length`/`pause`/`patterns`、`catchphrases[]`（口头禅）、`emotion_anger`/`emotion_tense`/`emotion_mask`/`emotion_sad`/`emotion_happy`（情绪变化）、`banned` |

MCP 条目另含：`formatted`（写作注入 Markdown，与 `character_fmt` 同形）、`character_relations`（画布人物↔人物边）、列表/关联时的 `order`。

**SidePlotMeta**

| 字段 | 说明 |
|------|------|
| `status` | `active` \| `resolved` \| `deferred`（空≈active） |
| `absorbed` | 是否已被「整理剧情」吸收 |

**KnowledgeCardPayload（树上知识卡，≠公共库）**

| 字段 | 说明 |
|------|------|
| `book_ids` | 关联公共库 id |
| `extract_prompt` | 检索/提取需求 |
| `extracted` | 可编辑特征文本（槽位卡写作注入常由 fmt 动态拼装） |
| `from_canon` | 遗留标记（旧「同步设定卡」）；写作不再按此注入公共库 |
| `slot` | 根固定槽位 id（空=普通知识卡）。世界观：`wv_core_laws` / `wv_spatiotemporal` / `wv_social_power` / `wv_history_culture` / `wv_existence` / `wv_info_flow`；子卡：`wv_axiom` / `wv_location` / `wv_race` / `wv_faction` / `wv_religion` / `wv_major_event`；故事规则父：`story_rules`；子：`sr_surface_setting` / `sr_story_engine` / `sr_fulfillment_system` / `sr_constraint_redlines`；**生成/精修枢纽**：`write_prompts`（根上固定，不可删；左右挂普通知识卡 = 生成 / 精修；本卡与子卡不进 `linked_knowledge` / 章节继承，由应用接到用户提示词后） |
| `core_laws` | 仅 `wv_core_laws`：`premise`、`taboos[]`、`power_system`、`power_expression` |
| `world_axiom` | 仅 `wv_axiom`：{name,statement,boundary,cost,mechanism} |
| `spatiotemporal` | 仅 `wv_spatiotemporal`：`premise`、`era`、`ecology`、`world_pattern`、`atmosphere` |
| `key_location` | 仅 `wv_location`：{name,features,terrain,faction} |
| `social_power` | 仅 `wv_social_power`：`premise`、`class_structure`、`political_system`、`power_visibility` |
| `world_race` | 仅 `wv_race`：{name,features,population,social_status} |
| `major_faction` | 仅 `wv_faction`：{name,faction_type,goal,means,power_base} |
| `existence` | 仅 `wv_existence`：`premise`、`death`、`calendar`、`lifespan`、`disease_reproduction` |
| `info_flow` | 仅 `wv_info_flow`：`premise`、`info_speed`、`info_barrier`、`rumor_truth`（旧键 `message_truth` 仍可读）、`knowledge_carrier` |
| `history_culture` | 仅 `wv_history_culture`：`premise`、`customs`、`economy`、`daily_slices` 等 |
| `world_religion` | 仅 `wv_religion` |
| `major_event` | 仅 `wv_major_event` |
| `surface_setting` / `story_engine` / `fulfillment_system` / `constraint_redlines` | 故事规则四卡结构化载荷。兑现系统含 `tension_archetypes[]`（旧键 `tension_circles` 仍可读） |

写作/大纲注入：挂在根或章上的世界观/故事规则槽不以可能过期的 `extracted` 截断为准，而由对应 `*_fmt` 拼完整正文（含子卡列表，若有）。MCP `linked_knowledge[].extracted` / `.formatted` 同此规则；有结构化载荷时一并摊到同级字段（如 `core_laws`、`world_axiom`、`surface_setting` 等）。

---

## 4. 公共知识库工具

### 4.1 `list_knowledge`

列出公共知识库。

**请求**

```json
{ "include_archived": false }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `include_archived` | boolean | 否 | 默认 `false`；为 `true` 时含已归档 |

**返回**：`KnowledgeBook[]`（见 §3.1）

---

### 4.2 `import_knowledge`

导入本地 `txt` / `epub`，切段并建本地向量索引。

**请求**

```json
{
  "path": "/Users/me/docs/lore.epub",
  "title": "可选书名",
  "genres": ["玄幻"]
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | string | 是 | 本机绝对路径 |
| `title` | string | 否 | 覆盖书名；省略则用文件/EPUB 元数据 |
| `genres` | string[] | 否 | 题材标签 |

**返回**：单个 `KnowledgeBook`

---

### 4.3 `search_knowledge`

向量 + 关键词检索公共库分段，结果供 **AI 二次加工后写回知识卡**（再调 `upsert_knowledge_card` 的 `extracted`）。完整导入全书用 `fill_knowledge_card`，不要把检索结果当全文。

**请求**

```json
{
  "query": "修炼体系与禁忌",
  "book_ids": ["kb-uuid-1"],
  "limit": 8,
  "chunk_cap": 600
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `query` | string | 是 | 检索句 |
| `book_ids` | string[] | 否 | 空=全部未归档库 |
| `limit` | integer | 否 | 默认 `8`，命中条数 |
| `chunk_cap` | integer | 否 | 默认 `600`，每段字数上限 |

**返回样本**

```json
[
  {
    "id": "kb-uuid#3",
    "title": "仙侠世界观设定集",
    "content": "……命中段落……"
  }
]
```

| 字段 | 说明 |
|------|------|
| `id` | `{book_id}#{chunk_idx}` |
| `title` | 书名 |
| `content` | 段落正文（已截断） |

---

### 4.4 `archive_knowledge`

归档 / 取消归档。

**请求**

```json
{ "id": "kb-uuid", "archived": true }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 知识库 id |
| `archived` | boolean | 否 | 默认 `true` |

**返回**：更新后的 `KnowledgeBook`

---

### 4.5 `delete_knowledge`

永久删除；**必须已归档**。

**请求**

```json
{ "id": "kb-uuid" }
```

**返回**

```json
{ "ok": true, "id": "kb-uuid" }
```

失败示例文案：`请先归档后再删除`

---

## 5. 小说工具

工作台**当前选中卡片**时，`novel_id` 与（写章/改卡类工具的）`node_id` **可省略**，服务端填入选中项。外部客户端仍可显式传入。未选中且未传参则报错。`upsert_*` 的 `node_id` 除外：省略表示**新建**，不会套用当前选中。

`node_id` 除树上真实 id 外，也可传 **「第N章」/ 章号 `3` / 唯一标题**（人物卡可用唯一人名）。写章工具裁剪后没有 `get_tree` 时仍可据此定位章节。找不到时错误文案会带可用章节列表。

### 5.1 `list_novels`

**请求**

```json
{ "include_archived": false }
```

**返回**：`NovelProject[]`

---

### 5.2 `create_novel`

**请求**

```json
{
  "title": "夜航船",
  "synopsis": "港口与记忆。",
  "chapter_count": 20,
  "word_count_min": 2000,
  "word_count_max": 3000
}
```

| 字段 | 类型 | 必填 | 默认 |
|------|------|------|------|
| `title` | string | 是 | — |
| `synopsis` | string | 否 | `""` |
| `chapter_count` | integer | 否 | `20` |
| `word_count_min` | integer | 否 | `2000` |
| `word_count_max` | integer | 否 | `3000` |
| `features` | object | 否 | 空对象（各数组 `[]`） |

**返回**：新建的 `NovelProject`（同时写入初始树，通常仅根节点）

---

### 5.2a `update_novel_features`

**请求**：`novel_id` + `features`（同 `NovelProject.features`）

**返回**：更新后的 `NovelProject`

---

### 5.3 `update_novel`

**请求**

```json
{
  "novel_id": "n-uuid",
  "title": "夜航船（修订）",
  "synopsis": "新简介",
  "chapter_count": 30,
  "word_count_min": 2500,
  "word_count_max": 3500
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `novel_id` | 否 | 工作台已选中时可省略 |
| `title` / `synopsis` / `chapter_count` / `word_count_*` / `features` | 否 | 只传要改的字段；`features` 为整对象替换 |

**返回**：更新后的 `NovelProject`（根节点 label/outline/字数计划会同步）

---

### 5.3a `get_worldview` / `ensure_worldview` / `apply_worldview` / `generate_worldview`

| 工具 | 作用 |
|------|------|
| `get_worldview` | 返回 `features` + `features_text` + 六卡/故事规则 `snapshot`（含 `story_rules_blocks`） |
| `ensure_worldview` | 补齐根上六张世界观卡 + 故事规则主卡与右侧四子卡空壳（不覆盖已有内容） |
| `apply_worldview` | 写入 `worldview` JSON（与 Chat 同结构）；支持 `story_rules_blocks` 四卡；落盘后排版 |
| `generate_worldview` | LLM 生成；**必须服从** `novel.features`；可选 `slot` 只补一张六卡的全部字段；默认 `apply=true` 落盘 |
| `get_story_rules` | 读取故事规则四卡 `blocks` + 各卡 `node_id`/`formatted` + 父卡；缺卡先 `ensure_worldview` |
| `apply_story_rules` | 写入四卡 `blocks` JSON（与 Chat / `generate_story_rules` 同形）；返回同 `get_story_rules` |
| `generate_story_rules` | LLM 生成故事规则四卡；默认 `apply=true` 落盘 |

`generate_worldview` 请求示例：

```json
{
  "novel_id": "n-uuid",
  "instruction": "按现有功能选项随机全新设定",
  "apply": true
}
```

只补一张卡（六卡之一，全部字段含子项）时加 `slot`（`wv_core_laws` 或 JSON 键 `core_laws` 等均可）：

```json
{
  "novel_id": "n-uuid",
  "slot": "wv_existence",
  "instruction": "补全存在基础全部字段",
  "apply": true
}
```

返回的 `worldview` 只含该键，`apply` 时不会覆盖其它卡。

**返回摘要（与实现一致）**

| 工具 | 返回 |
|------|------|
| `get_worldview` | `{ features, features_text, snapshot, story_rules_blocks }`（`snapshot` 含各 `wv_*` 与 `story_rules`；顶层另有一份 `story_rules_blocks`） |
| `ensure_worldview` | `{ ok, created }` |
| `apply_worldview` | `{ ok: true }` |
| `generate_worldview` | `{ assistant, applied, worldview }` |
| `get_story_rules` / `apply_story_rules` | `{ blocks, formatted, parent, cards[], ai_guidance }` |
| `generate_story_rules` | `{ assistant, applied, blocks }`（非完整 `get_story_rules`） |

---

### 5.3b `get_novel_info`

小说简介快照：书名/简介/字数计划，以及**根节点**关联的人物、剧情卡、知识卡，外加可选 **`volumes` 分卷摘要**。公共知识库不是小说设定源（请用树上知识卡）。  
了解一本书时优先用本工具，不必拉完整 `get_tree`。写某一章用 `get_chapter_write_context`。

根上关联 = `linked_*_ids` ∪ 连到根的边（顺序：列表在前，边补漏）。

**请求**

```json
{ "novel_id": "n-uuid" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `novel_id` | string | 否 | 工作台已选中时可省略（不是公共库 `book_id`） |

**返回样本**

```json
{
  "ai_guidance": {
    "features": "novel.features 为根节点功能选项……生成世界观必须遵守。",
    "plots": "linked_plots 为根节点关联的跨章剧情卡……分卷可选，见 volumes。",
    "characters": "linked_characters 含完整结构化 character（world_position / world_anchors / relations / core_belief / deep / voice.body_language 等）与 formatted 全文……无 personality",
    "knowledge": "linked_knowledge 为根上知识卡……公共知识库不是小说设定源。"
  },
  "novel": {
    "id": "n-uuid",
    "title": "夜航船",
    "synopsis": "港口与记忆。",
    "features": { "genres": ["urban"] },
    "word_count_min": 2000,
    "word_count_max": 3000,
    "chapter_count": 20
  },
  "features_text": "题材：都市",
  "node": {
    "id": "root-uuid",
    "kind": "novel",
    "label": "夜航船",
    "outline": "港口与记忆。",
    "linked_character_ids": ["char-1"],
    "linked_side_plot_ids": ["plot-a"],
    "linked_knowledge_ids": ["kn-1"]
  },
  "linked_character_ids": ["char-1"],
  "linked_side_plot_ids": ["plot-a"],
  "linked_knowledge_ids": ["kn-1"],
  "volumes": [
    {
      "id": "vol-1",
      "label": "第一卷",
      "volume": { "positioning": "启程…", "layer_setup": { "chapters": "1-10 章", "description": "…" } },
      "formatted": "## 本卷定位\n启程…\n…"
    }
  ],
  "linked_plots": [
    {
      "order": 0,
      "id": "plot-a",
      "label": "走私线",
      "outline": "…",
      "status": "active",
      "absorbed": false
    }
  ],
  "linked_characters": [
    {
      "order": 0,
      "id": "char-1",
      "label": "林晚",
      "true_name": "林晚",
      "character": {
        "gender": "女",
        "age": "24",
        "aliases": "",
        "world_anchors": { "embodies_law": "…", "shaped_by_law": "…", "will_challenge": "…" },
        "voice": {
          "catchphrases": ["这账不对"],
          "body_language": "说话时转笔",
          "emotion_anger": "…",
          "emotion_tense": "…",
          "emotion_mask": "…",
          "emotion_sad": "…",
          "emotion_happy": "…"
        },
        "core_belief": { "belief": "…", "author_verdict": "disprove", "source": "…" },
        "deep": {},
        "world_position": {},
        "relations": [],
        "constraints": "…"
      },
      "formatted": "## 真名\n林晚\n## 世界锚点\n…\n## 角色声线\n…",
      "character_relations": [{ "peer_id": "char-2", "peer_label": "张三", "relation": "旧识" }]
    }
  ],
  "linked_knowledge": [
    {
      "order": 0,
      "id": "kn-1",
      "label": "冷硬文风",
      "outline": "",
      "slot": "",
      "extract_prompt": "短句、少形容词",
      "extracted": "叙述用短句……",
      "formatted": "叙述用短句……",
      "book_ids": ["kb-uuid"]
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `novel` | 小说快照（简介、字数/章数计划、`features`；不含公共库绑定） |
| `features_text` | `novel.features` 可读摘要 |
| `node` | 根节点完整 `TreeNode` |
| `volumes` | 分卷摘要：`id`/`label`/`volume`（`VolumePayload`）/`formatted`（无分卷时为 `[]`；**不含** `outline`） |
| `linked_plots` / `linked_characters` / `linked_knowledge` | 根上关联卡；人物/知识条目形状同 §6.1 / `json_linked_knowledge` |

---

### 5.3c `get_selected_card`

工作台**当前点中的卡片**（进程内记忆，不落盘）。选中时前端写入 `novel_id` + `node_id`；本工具再读树上的活数据。离开工作台会清空。

用户说「这张卡 / 当前选中」时优先用本工具。写某一章用 `get_chapter_write_context`。改卡/细纲不要带正文。

**请求**

```json
{ "include_content": false }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `include_content` | boolean | 否 | 默认 `false`；章/剧情卡是否附带正文 Markdown |

**未选中**

```json
{ "selected": false }
```

节点已被删：`selected: false`，带 `novel_id` / `node_id` / `reason: "missing"`。

**选中时**

```json
{
  "selected": true,
  "novel_id": "n-uuid",
  "node": { "id": "ch-uuid", "kind": "chapter", "label": "第一章", "outline": "…" },
  "content": "# 第一章\n\n正文…"
}
```

| 字段 | 说明 |
|------|------|
| `node` | 完整 `TreeNode`（人物/知识卡字段在 `character` / `knowledge`） |
| `content` | 仅 `chapter` / `side_plot` 且 `include_content` 时为正文；否则 `""` |

人物写章注入请另调 `get_character_card`（含 `formatted` / `character_relations`）；本工具不额外包装。

---

### 5.4 `get_tree`

**请求**

```json
{ "novel_id": "n-uuid" }
```

**返回**：`NovelTree`（见 §3.3）

---

### 5.4b `layout_tree`

按四带（知识 | 人物 | 根+章节 | 剧情）重写节点 `position` 并落盘。**工作台已去掉一键排版**（卡片位置手动拖动保存）；本工具仍可供 MCP / 全局 Chat 整列。工作台打开时会收到 `novel-tree-changed` 刷新。

**请求**

```json
{ "novel_id": "n-uuid" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `novel_id` | string | 否 | 工作台已选中时可省略 |

**返回**

```json
{ "ok": true, "nodes": 12 }
```

---

### 5.5 `add_chapter`

在末章之后（按画布 y 排序）追加章节卡；没有章节时挂到根。边为上一节点 **bottom → 新章 top**（首章即根.bottom → 首章.top）。新卡放在宿主下方，不自动排版。  
`link_to` 可指向**分卷**：挂到该卷末章，卷下尚无章则挂分卷本身（`kind: "chapter"` 边）。

**请求**

```json
{
  "novel_id": "n-uuid",
  "title": "第一章 离港",
  "outline": "主角登船，遇旧识。",
  "link_to": "vol-uuid"
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `novel_id` | 否 | 工作台已选中时可省略 |
| `title` | 否 | 默认 `第 {n} 章` |
| `outline` | 否 | 大纲 |
| `link_to` | 否 | 宿主：volume / chapter / root；省略则挂末章或根 |

**返回**：新建的章节 `TreeNode`（`kind: "chapter"`）

---

### 5.5b `add_volume` / `get_volume` / `upsert_volume`

在根下追加**分卷**节点（可选）。边为根 **bottom → 分卷 top**，`kind: "volume"`。分卷可挂人物/剧情/知识卡；其下章节写作继承「根 ∪ 本卷」。

结构化字段（`VolumePayload`，深度合并，不打断关联）：

| 字段 | 说明 |
|------|------|
| `positioning` | 本卷定位（一句话） |
| `layer_setup` / `layer_confrontation` / `layer_resolution` | 三层结构：各含 `chapters`（如 `1-15 章`）+ `description` |
| `conflict_external` / `conflict_internal` / `conflict_deep` | 冲突层级 |
| `key_beats[]` | 关键节点：`order` / `cost` / `description` |

分卷内容**仅存**节点 `volume` 字段；**不写** `outline`。写作注入与 MCP 返回的 `formatted` 由 `volume_fmt` 动态拼装。画布摘要显示 `positioning`。

**`add_volume` 请求**

```json
{
  "novel_id": "n-uuid",
  "title": "第一卷 · 启程",
  "volume": {
    "positioning": "主角被迫离城",
    "layer_setup": { "chapters": "1-15 章", "description": "…" },
    "layer_confrontation": { "chapters": "16-30 章", "description": "…" },
    "layer_resolution": { "chapters": "31-40 章", "description": "…" },
    "conflict_external": "…",
    "conflict_internal": "…",
    "conflict_deep": "…",
    "key_beats": [{ "order": 1, "cost": "暴露身份", "description": "救人" }]
  }
}
```

**`get_volume`**：`node_id` 省略则返回 `{ "count", "volumes": [ entry… ] }`；指定 `node_id` 返回 `{ "volume": entry, "ai_guidance" }`。  
单条 `entry` 含：`id`、`label`、`volume`（全部结构化字段）、`formatted`（写作注入）、`linked_*` 摘要（人物/剧情/知识含完整 entry）。

**`upsert_volume`**：必填 `node_id`；可改 `title` 或任意 `volume` / 顶层字段（深度合并）。勿写 `outline`。不改动剧情/知识/人物关联。返回更新后的 `entry`（同 `volume_json_entry`）。

---

### 5.6 `update_chapter_outline`

更新章节或剧情卡的标题 / **简纲**（`outline`）；章节还可写 **细纲**（`detailed_outline: string[]`）。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "title": "第一章 离港（改）",
  "outline": "简纲要点…",
  "detailed_outline": ["开场：……", "冲突：……", "收束：……"]
}
```

**返回**：更新后的 `TreeNode`（含 `detailed_outline`）

---

### 5.6b `generate_detailed_outline`

由本章 **简纲**（`outline`）进化 **细纲**（`detailed_outline`）并写回树。条数按每章 `word_count_min/max` 估算（约 320 字/场面），避免条数过多导致正文超字。材料由服务端组装：简介 + 功能选项 + 人物 + 剧情要点 + **世界观/故事规则（与写章同级）**；细纲不得与之冲突。正文生成前若细纲为空会自动调用等价逻辑；本工具可强制重写。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid", "user_notes": "可选补充", "model": null }
```

**返回**

```json
{ "ok": true, "node_id": "ch-uuid", "label": "细纲", "detailed_outline": ["…"], "ai_guidance": "…" }
```

---

### 5.6c `regenerate_detailed_outline_item`

AI 重写本章细纲中的**一条**（0-based `index`），保留其余条目并写回树。工作台左栏每条旁的「AI 重写」同源。须遵守已链接世界观 / 故事规则 / 功能选项。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid", "index": 0, "user_notes": "可选补充", "model": null }
```

**返回**

```json
{
  "node_id": "ch-uuid",
  "index": 0,
  "item": "重写后的节拍…",
  "detailed_outline": ["…", "…"],
  "ai_guidance": "…"
}
```

---

### 5.7 `delete_node`

删除非根节点（章 / 人 / 剧情 / 知识卡）。删章会清理正文文件与相关边。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid" }
```

**返回**：删除后的完整 `NovelTree`

失败示例：`根节点不能删除`

---

### 5.8 `get_chapter_content`

读取章节/支线 Markdown **正文**（纯文本，无 JSON 包装）。  
**生成新章正文时不要调用**（用 `get_chapter_write_context` 取约束即可，避免旧稿污染生成条件）。**精修/改稿**已有正文时，在 `get_chapter_write_context` 之后调用本接口读旧稿。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid" }
```

**返回**：Markdown **纯文本**（无 JSON 包装）；无正文时为空字符串。

---

### 5.8b `get_chapter_info`

章节卡快照：**不含正文**；含大纲、关联人物、关联剧情卡、**本章直连知识卡**与 `ai_guidance`。  
写章请用 `get_chapter_write_context`（本接口知识**不含**根/卷继承）。

剧情：**章节仍继承根/分卷剧情卡**（只读）。**知识卡不向章/卷继承**（世界观、故事规则、根或卷直连知识只留在本节点）。  
- `volume`：父分卷 `{ id, label, volume, formatted }`（无则为 `null`；**不含**过时 `outline`）。  
- `linked_plots`：`inherited_from` = `root` | `volume` | `chapter`（兼容字段 `inherited_from_root`）；根/卷继承只读，**仅本章可排序**。  
- `linked_side_plot_ids`：仅本章剧情 id（不含根/卷继承）。  
- `linked_root_plot_ids` / `linked_volume_plot_ids`：根 / 分卷继承剧情 id。  
- `linked_knowledge_ids` / `linked_knowledge`：仅本章直连（条目含 `formatted`/`slot`/结构化载荷）。  
- `linked_characters`：与 §6.1 单人 entry 同形（含 `formatted`、`character_relations`、完整 `character`）。

返回中的 **`ai_guidance`** 供写章 AI 直接遵守：

1. **剧情**（`plots`）：根 / 分卷 / 本章分别按各自 order；不得混序或改写要点。  
2. **人物**（`characters`）：`linked_characters` 为本节**必须出场**的人物；须符合结构化字段与 `formatted`；关系见 `character_relations`。  
3. **知识卡**（`knowledge`）：`linked_knowledge` 为**本章直连**写作约束。根上世界观/故事规则不出现在本接口；写章用 `get_chapter_write_context`。  
4. **设定校对**（`lore`）：落盘前对照根层世界观六卡与故事规则；冲突则就地修订，无冲突勿改。  
5. **叙事连贯**（`narrative_coherence`）：时间因果、人物一致、细节统一、段落衔接、逻辑自洽、节奏情绪、信息有效；禁止不合理、不连贯、前后冲突的叙述。  
6. **禁止项**（`forbidden`）：逻辑冲突、矛盾事实、人设崩坏、擅自加设定、机械降神、硬切场景、说明文对话、重复注水、元叙述等。  
7. **字数**（`length`）：瞄准 `word_count_min`/`max` 中位；±60 不是反复重写门槛。  
8. **输出**（`output`）：只输出 Markdown 正文，不要清单/自我评价。  
9. **正文用法**（`content_usage`）：写章用 `get_chapter_write_context`。本接口不含正文。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `novel_id` | string | 否 | 工作台已选中时可省略 |
| `node_id` | string | 否 | 章节（或支线）节点 id、「第N章」或唯一标题；工作台已选中时可省略 |

**返回样本**

```json
{
  "ai_guidance": {
    "plots": "linked_plots 的 inherited_from 为 root|volume|chapter……",
    "characters": "linked_characters 为本章必须出场的人物；须符合 character 结构化字段与 formatted；见 character_relations……",
    "knowledge": "linked_knowledge 为本章写作硬约束……优先 extracted/formatted……",
    "lore": "落盘前对照根层世界观六卡与故事规则……冲突则就地修订……",
    "narrative_coherence": "时间与因果、人物一致、细节统一、段落衔接……",
    "forbidden": "严禁前后逻辑冲突、人设崩坏、机械降神……",
    "length": "字数须符合 get_novel_info 的 word_count_min/max……",
    "output": "只输出本章 Markdown 正文……",
    "content_usage": "本接口不含正文。生成新章……精修/改稿……get_chapter_content……"
  },
  "node": {
    "id": "ch-uuid",
    "kind": "chapter",
    "label": "第一章",
    "outline": "离港…",
    "detailed_outline": ["码头遇雨", "旧识拦路", "登船离港"],
    "linked_character_ids": ["char-1"],
    "linked_side_plot_ids": ["plot-a", "plot-b"],
    "linked_knowledge_ids": ["kn-1"],
    "word_count": 2100
  },
  "linked_side_plot_ids": ["plot-a"],
  "linked_root_plot_ids": ["plot-root"],
  "linked_volume_plot_ids": ["plot-vol"],
  "volume": {
    "id": "vol-1",
    "label": "第一卷",
    "volume": { "positioning": "启程…" },
    "formatted": "## 本卷定位\n启程…"
  },
  "linked_knowledge_ids": ["kn-root", "kn-1"],
  "linked_plots": [
    {
      "order": 0,
      "inherited_from": "root",
      "inherited_from_root": true,
      "id": "plot-root",
      "label": "全书主线",
      "outline": "…",
      "status": "active",
      "absorbed": false
    },
    {
      "order": 0,
      "inherited_from": "volume",
      "inherited_from_root": false,
      "id": "plot-vol",
      "label": "本卷线",
      "outline": "…",
      "status": "active",
      "absorbed": false
    },
    {
      "order": 0,
      "inherited_from": "chapter",
      "inherited_from_root": false,
      "id": "plot-a",
      "label": "走私线",
      "outline": "…",
      "status": "active",
      "absorbed": false
    }
  ],
  "linked_characters": [
    {
      "order": 0,
      "id": "char-1",
      "label": "林晚",
      "true_name": "林晚",
      "character": {
        "gender": "女",
        "age": "24",
        "world_anchors": { "embodies_law": "…", "will_challenge": "…" },
        "voice": { "catchphrases": ["……"], "emotion_anger": "…", "emotion_happy": "…" },
        "constraints": "…"
      },
      "formatted": "## 真名\n林晚\n…",
      "character_relations": []
    }
  ],
  "linked_knowledge": [
    {
      "order": 0,
      "id": "kn-1",
      "label": "冷硬文风",
      "outline": "",
      "slot": "",
      "extract_prompt": "短句、少形容词、禁用网络梗",
      "extracted": "叙述用短句；对话克制……",
      "formatted": "叙述用短句；对话克制……",
      "book_ids": ["book-uuid"]
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `ai_guidance.plots` | 写章须严格按剧情卡顺序与内容 |
| `ai_guidance.characters` | 关联人物必须出场，且遵守结构化字段 / `formatted` / `character_relations` |
| `ai_guidance.knowledge` | 关联知识卡为写作手法/文风等硬约束（`extracted`/`formatted` 及结构化槽位字段） |
| `ai_guidance.lore` | 落盘前对照根层世界观/故事规则；冲突则就地修订，无冲突勿改 |
| `ai_guidance.narrative_coherence` | 叙事合理连贯：时间因果、人设一致、细节统一、段落衔接、逻辑自洽等 |
| `ai_guidance.forbidden` | 禁止逻辑冲突、矛盾事实、人设崩坏、擅自加设定、机械降神、元叙述等 |
| `ai_guidance.length` | 瞄准每章目标中位；`in_band` 后禁止再为篇幅重写 |
| `ai_guidance.output` | 只输出 Markdown 正文 |
| `ai_guidance.content_usage` | 本接口不含正文；生成须先有细纲（空则 `generate_detailed_outline`）；生成勿调 `get_chapter_content`；精修才另读旧稿；落盘前对照 `lore` |
| `node` | 完整 `TreeNode`（含 `detailed_outline`） |
| `volume` | 父分卷摘要：`id`/`label`/`volume`/`formatted`；无父卷为 `null` |
| `linked_side_plot_ids` | 仅本章剧情 id（可排序；不含根继承） |
| `linked_root_plot_ids` | 根节点继承的剧情 id |
| `linked_volume_plot_ids` | 分卷继承的剧情 id |
| `linked_plots[].order` | 根 / 卷 / 本章各自从 `0` 起；看 `inherited_from` |
| `linked_plots[].inherited_from` | `root` \| `volume` \| `chapter` |
| `linked_characters` | 本章必出人物（同 §6.1 entry，含 `order`） |
| `linked_knowledge` / `linked_knowledge_ids` | 仅本章直连（含 `formatted`/`slot`/结构化载荷） |

工作台：画布把知识卡连到根 / 分卷 / 章节会写入对应 `linked_knowledge_ids`；左侧可拖拽排序**本节点直连**知识。世界观/故事规则与根、卷知识不向章继承。

**全局 Chat「生成/重写第 N 章」**：系统指令要求先 `get_chapter_write_context` 并遵守返回的 `ai_guidance`（已有根 → `include_root=false`；同卷已有 → `include_volume=false`；沿用当前 session）。Chat 写章轮次只暴露 `get_chapter_write_context` / `generate_detailed_outline` / `get_chapter_content` / `set_chapter_content`。**改人物/剧情/知识卡**只暴露 `get_selected_card` / `get_character_card` / `upsert_*` / `fill_knowledge_card` / `link_nodes` / `unlink_nodes`；**生成/重写细纲**只暴露 `get_selected_card` / `generate_detailed_outline` / `regenerate_detailed_outline_item` / `update_chapter_outline`。新生成禁止 `get_chapter_content`；改稿再读旧稿。`set_chapter_content` / `upsert_*` 成功后历史里去掉整段正文或整卡 JSON；load 时只保留最近 2 轮 user + 更早轮次摘要。绑定小说且非写章时按章节记忆做 `dynamic_context`（不含公共库）。`/novel` skill 只在用户显式输入时注入并放开全部工具。Chat 按设置里的 **AI API** 协议选 rig 客户端（Completions / Responses / DeepSeek / BigModel / Anthropic / Gemini 等；阿里云走 Completions）。详见 `Project.md` §6。

---

### 5.8c `get_chapter_write_context`

写章专用材料包：**不含正文**。一次返回根（可选）+ 分卷（可选）+ 本章；人物/剧情/知识按 id **去重（先到优先）**。同一 Chat session 已提交过根/卷时关掉对应 include，避免重复 token。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "include_root": true,
  "include_volume": true
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `novel_id` | string | 否 | 工作台已选中时可省略 |
| `node_id` | string / integer | 否 | 章节 id、「第N章」、章号或唯一标题；工作台已选中时可省略 |
| `include_root` | bool | 否 | 默认 `true`。本会话已有根材料时传 `false`（仍从章/卷结果中剔除根卡 id） |
| `include_volume` | bool | 否 | 默认 `true`。同卷已提交时传 `false`。无分卷则 `volume` 为 null |

**返回**（紧凑 JSON）

```json
{
  "included": { "root": true, "volume": false, "chapter": true },
  "root": {
    "title": "书名",
    "synopsis": "…",
    "features_text": "…",
    "word_count_min": 3000,
    "word_count_max": 4000,
    "linked_characters": [{ "id": "c1", "label": "陈默", "formatted": "…" }],
    "linked_plots": [{ "id": "p1", "label": "主线", "outline": "…", "scope": "root" }],
    "linked_knowledge": [{ "id": "k1", "label": "核心法则", "slot": "wv_core_laws", "formatted": "…", "scope": "root" }]
  },
  "volume": null,
  "chapter": {
    "id": "ch-uuid",
    "label": "第19章",
    "outline": "简纲",
    "detailed_outline": ["细纲1"],
    "word_count_min": 3000,
    "word_count_max": 4000,
    "linked_characters": [],
    "linked_plots": [],
    "linked_knowledge": []
  },
  "ai_guidance": {}
}
```

人物条目仅 `id`/`label`/`formatted`/`character_relations`（无完整 `character` 再拷一份）。知识仅 `formatted`（不再重复 `extracted`）。`include_root=false` 时 `ai_guidance` 为短 note。

根上 `slot=write_prompts`（生成/精修）及其左右子卡**不**出现在各层 `linked_knowledge`。Chat 写章由应用把对应侧知识接到用户消息后；后端 `generate_chapter` / `refine_chapter` 接到 `user_brief` 后。不要把这些卡再塞进写章上下文。

---

### 5.9 `set_chapter_content`

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "content": "# 第一章\n\n正文……\n"
}
```

**返回**

```json
{ "ok": true, "word_count": 1234, "word_count_min": 2000, "word_count_max": 3000, "in_band": true, "ai_guidance": "字数已在允许区间，禁止再为篇幅改正文。" }
```

| 字段 | 说明 |
|------|------|
| `word_count` | 非空白字符数（写入树节点） |
| `word_count_min` / `word_count_max` | 本书每章目标 |
| `in_band` | 是否落在目标 ±60。`true` 时禁止再为篇幅重写 |
| `ai_guidance` | 交卷/只删冗一轮 |

仅允许 `chapter` / `side_plot` 节点。

---

### 5.9a `get_chapter_memory`

读取单章**章节记忆**（蒸馏事实条目，非正文）。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid" }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `novel_id` | string | 否 | 工作台已选中时可省略 |
| `node_id` | string | 否 | 章节节点；工作台已选中时可省略 |

**返回**

```json
{ "node_id": "ch-uuid", "items": ["林晚离港，与父亲决裂。", "…"] }
```

---

### 5.9b `list_chapter_memory`

全书已有章节记忆，按结构树章节顺序分组（无记忆的章不出现）。

**请求**

```json
{ "novel_id": "n-uuid" }
```

**返回**

```json
[
  { "node_id": "ch-1", "label": "第一章", "items": ["…"] },
  { "node_id": "ch-2", "label": "第二章", "items": ["…"] }
]
```

---

### 5.9c `set_chapter_memory`

手动覆盖或清空某章记忆（`items` 为空数组则清除 SQLite 列表 + Lance 向量）。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "items": ["事实一", "事实二"]
}
```

**返回**

```json
{ "ok": true }
```

---

### 5.9d `regenerate_chapter_memory`

按磁盘上当前正文用 LLM 抽取并**覆盖**本章记忆。正文为空时报错。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `user_notes` | string | 否 | 抽取注意事项（可空） |
| `memory_node_ids` | string[] | 否 | 其他章 `node_id`，注入其记忆作对照去重 |

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "user_notes": "只记人物关系与关键道具",
  "memory_node_ids": ["ch-1"]
}
```

**返回**

```json
{ "items": ["…", "…"] }
```

仅 `chapter` 节点；模拟模式（无真实 LLM）不写入并报错。

---

### 5.9e 分镜头 / ComfyUI MiniMax

镜头存在 `novels/{id}/shots/{node_id}.json`，不进结构树。设置：`comfyui_url`（默认 `http://127.0.0.1:8188`）、`comfyui_workflow`（ComfyUI「导出（API）」JSON）、可选 `comfyui_prompt_node`。

| 工具 | 作用 |
|------|------|
| `get_chapter_shots` | 读镜头列表（`action` / `camera` / `dialogue` / `duration_sec` / `comfy_prompt`） |
| `set_chapter_shots` | 整表覆盖（**一章多镜**，`shots` 数组）；`shots: []` 清空。MCP 拆镜/写提示词后必须用此写入 |
| `split_chapter_shots` | **不调用应用 LLM、不落盘**。返回 `body` / `characters` / `existing_shots` / `ai_guidance`；由客户端拆镜后 `set_chapter_shots`（需非空正文） |
| `generate_shot_comfy_prompts` | **不调用应用 LLM、不落盘**。返回已有 `shots` + `characters` + `ai_guidance`；由客户端写 `comfy_prompt` 后 `set_chapter_shots` |
| `submit_chapter_shots_comfyui` | `POST {comfyui_url}/prompt` 入队；不下载成片。含 Prompt Chain 的工作流一次提交全部镜头，否则每镜一条 |

工作台「按正文拆镜头 / 生成提示词」仍走应用内置模型。MCP 路径用客户端流量。

**`get` / `set` 返回**

```json
{
  "node_id": "ch-uuid",
  "shots": [
    {
      "id": "s-uuid",
      "order": 1,
      "action": "推门进屋",
      "camera": "肩后跟拍",
      "dialogue": "谁？",
      "duration_sec": 8,
      "comfy_prompt": "A man pushes a wooden door…"
    }
  ]
}
```

**`split_chapter_shots` 返回（未保存）**

```json
{
  "node_id": "ch-uuid",
  "label": "第一章",
  "body": "……",
  "characters": "- 李四\\n……",
  "existing_shots": [],
  "saved": false,
  "shot_fields": ["id", "order", "action", "camera", "dialogue", "duration_sec", "comfy_prompt"],
  "min_shots": 2,
  "typical_shots": "6-20",
  "ai_guidance": "……一章必须拆成多条……禁止把整章收成 1 条……再 set_chapter_shots……"
}
```

**`generate_shot_comfy_prompts` 返回（未保存）** `{ "node_id", "label", "characters", "shots", "saved": false, "ai_guidance" }`

**`submit` 返回** `{ "queued": 3, "prompt_ids": ["…"], "mode": "each"|"chain", "url": "http://127.0.0.1:8188" }`

工作流须为 API 格式对象（节点 id → `{class_type, inputs}`）。自动挑 MiniMax / CLIP 的 `text`/`prompt`/`prompts` 字段。

---

## 6. 卡片与关联

### 6.1 `get_character_card`

读取**完整人物卡**：结构化 `character`（含 `world_position`、`world_anchors`、`relations`、`core_belief`、`deep`、`voice` 等）、写作注入用 `formatted` Markdown，以及画布上的人物↔人物关系 `character_relations`。

| 参数 | 说明 |
|------|------|
| `novel_id` | 可省略（当前选中书） |
| `node_id` | 人物节点 id 或唯一人名；**省略则只返回 id/label 列表**（读完整卡必须带 `node_id`） |

**返回（单人）**

```json
{
  "character": {
    "id": "char-uuid",
    "label": "李四",
    "true_name": "李四",
    "character": {
      "gender": "男",
      "aliases": "…",
      "world_position": { "faction": "商会", "social_role": "账房" },
      "world_anchors": {
        "embodies_law": "等价交换",
        "embodies_note": "…",
        "shaped_by_law": "信息有价",
        "shaped_by_note": "…",
        "will_challenge": "皇权专营"
      },
      "relations": [{ "name": "张三", "relation": "旧识", "definition": "…", "default_attitude": "…", "hidden_tension": "…" }],
      "core_belief": { "belief": "…", "author_verdict": "disprove", "source": "…" },
      "deep": { "…": "…" },
      "voice": {
        "positioning": "…",
        "body_language": "说话时转笔",
        "catchphrases": ["这账不对"],
        "emotion_anger": "…",
        "emotion_tense": "…",
        "emotion_mask": "…",
        "emotion_sad": "…",
        "emotion_happy": "…",
        "banned": "…"
      },
      "constraints": "…"
    },
    "formatted": "## 真名\n李四\n## 世界锚点\n…\n## 角色声线\n- 口头禅：这账不对\n…",
    "character_relations": [{ "peer_id": "char-2", "peer_label": "张三", "relation": "旧识" }]
  }
}
```

单人查询**不含** `order`；`get_novel_info` / `get_chapter_info` 的 `linked_characters` 以及全书列表条目含 `order`（从 0 起）。

**返回（全书列表）**：`{ "count": N, "characters": [{ "id", "label" }], "ai_guidance": "…" }`

`get_novel_info` / `get_chapter_info` 的 `linked_characters` 条目形状与单人相同（含 `formatted`、完整 `character`）。

写章注入请优先读 `formatted`；结构化读写用 `character.*`。`get_selected_card` 仅返回原始 `TreeNode`（无 `formatted`），人物请改用本工具。

---

### 6.2 `upsert_character_card`

无 `node_id` 则新建；有则更新。新建默认挂**章节卡**（同前）。新卡不自动排版，需要整列时用 `layout_tree`。

**更新是深度合并**：省略的结构化字段保留原值；可传：
- `character`：`CharacterCard` 本体，**或** `get_character_card` 返回的整条 entry（含 `formatted` 的包装会自动摊平）
- 顶层 `world_position` / `world_anchors` / `relations` / `core_belief` / `deep` / `voice`
- 扁平字段 `gender` / `age` / `body_language` / …（`body_language` 写入 `voice.body_language`）。**不要传 `personality`**（忽略）

更新时可省略 `name`（保留原真名）。新建必须传 `name`。

**请求（新建，完整结构化）**

```json
{
  "novel_id": "n-uuid",
  "name": "李四",
  "character": {
    "gender": "男",
    "age": "外表中年，实际800岁",
    "aliases": "老李",
    "world_position": {
      "birth_class": "贱籍",
      "faction": "商会",
      "social_role": "账房",
      "conflict_stance": "反对苛税"
    },
    "world_anchors": {
      "embodies_law": "等价交换",
      "embodies_note": "每笔账都要还",
      "shaped_by_law": "信息有价",
      "shaped_by_note": "情报黑市长大",
      "will_challenge": "皇权专营"
    },
    "relations": [
      {
        "name": "张三",
        "relation": "旧识",
        "definition": "可利用的棋子",
        "default_attitude": "热情但贪婪",
        "hidden_tension": "想套出秘密"
      }
    ],
    "core_belief": {
      "belief": "钱能买命",
      "author_verdict": "disprove",
      "source": "早年饥荒"
    },
    "deep": { … },
    "voice": {
      "catchphrases": ["这账不对"],
      "body_language": "说话时转笔",
      "emotion_anger": "压低嗓门翻旧账",
      "emotion_tense": "句式变短",
      "emotion_mask": "改用敬语",
      "emotion_sad": "少说话",
      "emotion_happy": "爱开玩笑",
      "banned": "网络流行语"
    }
  },
  "link_to": "ch-uuid"
}
```

**请求（更新，部分字段）**

```json
{
  "node_id": "char-uuid",
  "character": { "gender": "女", "world_position": { "faction": "商会" } }
}
```

或顶层：

```json
{ "node_id": "char-uuid", "world_position": { "faction": "商会" }, "gender": "女" }
```

**返回**

```json
{
  "ok": true,
  "created": false,
  "node_id": "char-uuid",
  "label": "李四",
  "kind": "character",
  "ai_guidance": "已写入。勿把整卡贴回对话；再改同一张卡继续 upsert，省略未改字段。"
}
```

---

### 6.3 `upsert_plot_card`

**请求**

```json
{
  "novel_id": "n-uuid",
  "title": "走私线",
  "outline": "港口走私与海关对峙。",
  "status": "active",
  "link_to": "root"
}
```

| 字段 | 说明 |
|------|------|
| `status` | `active` \| `resolved` \| `deferred` |
| `link_to` | 章或根的**节点 id**；也可写 `root` / `novel`（解析成小说根的真实 id，不是字面 `"root"`）。**新建且省略时挂当前选中章，否则末章，再否则根**。边为 **章.right → 剧情.left** |

**返回**：`{ "ok": true, "node_id", "label", "kind": "side_plot", "ai_guidance" }`（不回整张 TreeNode）

---

### 6.4 `upsert_knowledge_card`

树上知识卡（非公共库本体）。

**更新是部分字段**：省略的 `book_ids` / `extract_prompt` / `extracted` **保留原值**（便于 AI 只写回 `extracted`）。

世界观 / 故事规则固定槽：传 `slot`（如 `wv_core_laws`、`sr_surface_setting`）可省略 `node_id`；传结构化 payload（`core_laws`、`surface_setting` 等）会自动同步 `extracted`（与写作注入同形）。固定槽不会重复新建，也不会被挂到章节下。`slot=write_prompts` 同样就地更新、不可当新卡挂到章下。

选库后两条路：

| 方式 | 工具 |
|------|------|
| 完整导入关联公共库 | 先保证卡上有 `book_ids`，再 `fill_knowledge_card` |
| AI 处理后写回 | `search_knowledge` 至多一轮 → 整理要点 → `upsert_knowledge_card`（用户点「是」后立刻建卡，禁止再检索） |

**请求**

```json
{
  "novel_id": "n-uuid",
  "title": "修炼设定卡",
  "book_ids": ["kb-uuid"],
  "extract_prompt": "境界与代价",
  "extracted": "……特征摘要……",
  "link_to": "root"
}
```

**返回**：

```json
{
  "ok": true,
  "created": false,
  "node_id": "kn-uuid",
  "label": "修炼设定卡",
  "kind": "knowledge",
  "slot": "",
  "ai_guidance": "已写入。勿把整卡贴回对话；再改同一张卡继续 upsert，省略未改字段。"
}
```

请求可带结构化载荷（`core_laws` / `surface_setting` / …），写入后自动同步 `extracted`；回执不再回这些字段。`slot=story_rules` 同理。

新建且未传 `link_to` 时挂当前选中章，否则末章，再否则小说根。边为 **章.left ← 知识.right**。`link_to` 为 `"root"` / `"novel"` 时解析为根节点真实 id（树里的根 id 通常是 UUID，不是 `"root"`）。

---

### 6.4.1 `fill_knowledge_card`

把知识卡关联的公共库**完整导入**到 `extracted`（有字数上限，默认 14000；优先分析/目录块）。AI 提炼不要走此工具。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "kb-card-uuid",
  "book_ids": ["kb-uuid"],
  "max_chars": 14000
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `novel_id` / `node_id` | 否 | 工作台已选中该知识卡时可省略 |
| `book_ids` | 否 | 有则先写入卡片再导入；否则用卡上已有 `book_ids` |
| `max_chars` | 否 | 导入正文上限，默认 `14000` |

**返回**：`{ "ok": true, "node_id", "label", "extracted_chars", "ai_guidance" }`（不回整张 TreeNode / extracted 正文）

---

### 6.4.2 公共知识卡（跨小说目录）

树上知识卡默认只属于一本小说。用户可把某张卡**复制进目录**，再在其他小说里挂上（再次复制，不共享编辑；版本号以后再做）。

**不是** Library 公共知识库（`list_knowledge` 那些书）。

#### `list_public_knowledge_cards`

```json
{ "include_archived": false }
```

默认不含已归档。返回 `PublicKnowledgeCard[]`：`id`、`title`、`book_ids`、`extract_prompt`、`extracted`、`created_at`、`updated_at`、`archived`。

#### `archive_public_knowledge_card`

```json
{ "id": "pkc-uuid", "archived": true }
```

只改目录项；树上已挂副本不变。

#### `upsert_public_knowledge_card`

写入目录，不自动挂到小说。可只填标题和手写规则（`extracted` 可空或几行约束），不必关联公共库。

```json
{ "title": "文风卡", "extracted": "……", "book_ids": [], "extract_prompt": "" }
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `id` | 否 | 有则更新该目录项 |
| `title` / `extracted` / `book_ids` / `extract_prompt` | 否 | 省略时可从工作台**当前选中的树上知识卡**抄过来（`novel_id`/`node_id` 可省略） |
| `novel_id` / `node_id` | 否 | 仅当要从树上知识卡复制时使用 |

**返回**：`PublicKnowledgeCard`

#### `add_public_knowledge_card`

把目录里的一张卡**复制**成树上知识卡，并挂到指定/当前选中节点（章、根，或人物/剧情/知识卡的宿主）。

```json
{ "public_id": "pkc-uuid", "link_to": "chapter-uuid" }
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `public_id` | 是 | 目录 id |
| `novel_id` | 否 | 省略则用工作台当前小说 |
| `link_to` | 否 | 章/根 id，或 `root`/`novel`；省略则用当前选中 |

**返回**：新建的知识 `TreeNode`

---

### 6.5 `link_nodes`

**请求**

```json
{
  "novel_id": "n-uuid",
  "source_id": "root",
  "target_id": "char-uuid",
  "kind": "character"
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `novel_id` | 否 | 工作台已选中时可省略 |
| `source_id` / `target_id` | 是 | 两端节点 |
| `kind` | 否 | `character` \| `side_plot` \| `knowledge` \| `chapter`；空则按节点类型推断 |

会写边，并更新宿主 `linked_*_ids`。

**返回**

```json
{
  "ok": true,
  "host": "root",
  "card": "char-uuid",
  "kind": "character"
}
```

---

### 6.6 `unlink_nodes`

世界观 / 故事规则卡的连线不可断开。`write_prompts`（生成/精修）**只**锁与根的那条边；左右子卡可以断开。

**按边 id**

```json
{ "novel_id": "n-uuid", "edge_id": "e-root-char-uuid" }
```

**按两端**

```json
{
  "novel_id": "n-uuid",
  "source_id": "root",
  "target_id": "char-uuid"
}
```

**返回**

```json
{ "ok": true }
```

---

## 7. curl 速查

```bash
# 列出工具
curl -s http://127.0.0.1:17832/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'

# 检索知识库
curl -s http://127.0.0.1:17832/mcp \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc":"2.0","id":2,"method":"tools/call",
    "params":{
      "name":"search_knowledge",
      "arguments":{"query":"修炼体系","limit":5}
    }
  }'
```

---

## 8. 维护说明

数据格式（`models.rs`、前端载荷、写作注入）变更时，**同一 PR** 须同步 MCP。完整检查清单见 `.cursor/rules/mcp-sync.mdc`。

| 改动 | 同步 |
|------|------|
| `src-tauri/src/mcp.rs` 工具名 / arguments / 返回 JSON | **本文** + `src-tauri/skills/novel/SKILL.md` + `skills.rs` 工具名测试 |
| `CharacterCard` / `KnowledgeCardPayload` / `NovelFeatures` 等树上载荷 | 上表 + `json_linked_*` / `get_*` / `upsert_*` / `patch_*` + 对应 `*_fmt.rs` |
| 章节 AI 预生成 / 精修 / 记忆策略 | `Project.md`（`project-md-sync.mdc`） |
| 应用内 Tauri `invoke` 命令 | `src/lib/api.ts`（非 MCP，但常与载荷同改） |
