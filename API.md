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

兼容说明：Cursor / ADK `McpHttpClientBuilder` 等标准 Streamable HTTP 客户端即可；勿再假定「纯手写 JSON-RPC + GET 说明页」。

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

**`kind`（节点）**：`novel` \| `chapter` \| `character` \| `side_plot` \| `knowledge`

**`kind`（边）**：`chapter` \| `character` \| `side_plot` \| `knowledge` 等

| TreeNode 字段 | 说明 |
|---------------|------|
| `label` | 显示名 / 章标题 |
| `outline` | 大纲或简介文本 |
| `character` | 人物卡载荷（仅 `character`） |
| `knowledge` | 树上知识卡载荷（仅 `knowledge`） |
| `side_plot` | 剧情卡元数据（仅 `side_plot`） |
| `linked_*_ids` | 挂在本章/根上的卡 id |
| `word_count` | 本章已写正文非空白字数 |
| `word_count_min/max`、`chapter_count` | 根节点计划字段 |

**CharacterCard**

| 字段 | 说明 |
|------|------|
| `role` | 身份 |
| `gender` | 性别 |
| `alignment` | 阵营 |
| `personality` | 性格 |
| `style` | 行事风格 |
| `motto` | 座右铭 |

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
| `extracted` | 可编辑特征文本 |
| `from_canon` | 遗留标记（旧「同步设定卡」）；写作不再按此注入公共库 |

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

**返回**：新建的 `NovelProject`（同时写入初始树，通常仅根节点）

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
| `title` / `synopsis` / `chapter_count` / `word_count_*` | 否 | 只传要改的字段 |

**返回**：更新后的 `NovelProject`（根节点 label/outline/字数计划会同步）

---

### 5.3b `get_novel_info`

小说简介快照：书名/简介/字数计划，以及**根节点**关联的人物、剧情卡、知识卡，外加可选 **`volumes` 分卷摘要**。公共知识库不是小说设定源（请用树上知识卡）。  
了解一本书时优先用本工具，不必拉完整 `get_tree`。写某一章仍用 `get_chapter_info`。

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
    "plots": "linked_plots 为根节点关联的跨章剧情卡……分卷可选，见 volumes。",
    "characters": "linked_characters 为根上贯穿人物……",
    "knowledge": "linked_knowledge 为根上知识卡……公共知识库不是小说设定源。"
  },
  "novel": {
    "id": "n-uuid",
    "title": "夜航船",
    "synopsis": "港口与记忆。",
    "word_count_min": 2000,
    "word_count_max": 3000,
    "chapter_count": 20
  },
  "node": {
    "id": "root",
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
    { "id": "vol-1", "label": "第一卷", "outline": "启程…" }
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
      "character": { "role": "女主", "personality": "…", "motto": "", "gender": "女", "style": "", "alignment": "" }
    }
  ],
  "linked_knowledge": [
    {
      "order": 0,
      "id": "kn-1",
      "label": "冷硬文风",
      "outline": "",
      "extract_prompt": "短句、少形容词",
      "extracted": "叙述用短句……",
      "book_ids": ["kb-uuid"]
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `novel` | 小说快照（简介、字数/章数计划；不含 `knowledge_ids` / `canon_mode`） |
| `node` | 根节点完整 `TreeNode` |
| `volumes` | 分卷摘要列表（按画布 y；无分卷时为空数组） |
| `linked_plots` / `linked_characters` / `linked_knowledge` | 根上关联卡，结构同 `get_chapter_info` |

---

### 5.3c `get_selected_card`

工作台**当前点中的卡片**（进程内记忆，不落盘）。选中时前端写入 `novel_id` + `node_id`；本工具再读树上的活数据。离开工作台会清空。

用户说「这张卡 / 当前选中」时优先用本工具。写某一章仍用 `get_chapter_info`。

**请求**

```json
{ "include_content": true }
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `include_content` | boolean | 否 | 默认 `true`；章/剧情卡是否附带正文 Markdown |

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

---

### 5.4 `get_tree`

**请求**

```json
{ "novel_id": "n-uuid" }
```

**返回**：`NovelTree`（见 §3.3）

---

### 5.4b `layout_tree`

工作台「一键排版」的 MCP 版：按四带（知识 | 人物 | 根+章节 | 剧情）重写节点 `position` 并落盘。工作台打开时会收到 `novel-tree-changed` 刷新。

新建卡片时服务端已自动排一次；本工具用于手动挪卡之后再整列。

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

在末章之后（按画布 y 排序）追加章节卡；没有章节时挂到根。边为上一节点 **bottom → 新章 top**（首章即根.bottom → 首章.top）。写入后自动一键排版。  
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

### 5.5b `add_volume`

在根下追加**分卷**节点（可选）。边为根 **bottom → 分卷 top**，`kind: "volume"`。分卷可挂人物/剧情/知识卡；其下章节写作继承「根 ∪ 本卷」。

**请求**

```json
{
  "novel_id": "n-uuid",
  "title": "第一卷 · 启程",
  "outline": "本卷主线…"
}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `novel_id` | 否 | 工作台已选中时可省略 |
| `title` | 否 | 默认 `第 {n} 卷` |
| `outline` | 否 | 分卷纲要 |

**返回**：新建的分卷 `TreeNode`（`kind: "volume"`）

---

### 5.6 `update_chapter_outline`

更新章节或剧情卡的标题/大纲。

**请求**

```json
{
  "novel_id": "n-uuid",
  "node_id": "ch-uuid",
  "title": "第一章 离港（改）",
  "outline": "更新后的大纲要点…"
}
```

**返回**：更新后的 `TreeNode`

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
**生成新章正文时不要调用**（用 `get_chapter_info` 取约束即可，避免旧稿污染生成条件）。**精修/改稿**已有正文时，在 `get_chapter_info` 之后调用本接口读旧稿。

**请求**

```json
{ "novel_id": "n-uuid", "node_id": "ch-uuid" }
```

**返回**：Markdown **纯文本**（无 JSON 包装）；无正文时为空字符串。

---

### 5.8b `get_chapter_info`

章节卡快照：**不含正文**；含大纲、关联人物、关联剧情卡、**关联知识卡**与 `ai_guidance`。  
生成新章：仅依据本接口 → 写全新正文 → `set_chapter_content`。精修/改稿：本接口 + `get_chapter_content` → 改稿 → `set_chapter_content`。

剧情：**所有章节继承根节点关联的剧情卡与知识卡；若章节挂在分卷下，另继承该分卷关联卡**。  
- `volume`：父分卷 `{ id, label, outline }`（无则为 `null`）。  
- `linked_plots`：`inherited_from` = `root` | `volume` | `chapter`（兼容字段 `inherited_from_root`）；根/卷继承只读，**仅本章可排序**。  
- `linked_side_plot_ids`：仅本章剧情 id（不含根/卷继承）。  
- `linked_root_plot_ids` / `linked_volume_plot_ids`：根 / 分卷继承剧情 id。  
- `linked_knowledge_ids` / `linked_knowledge`：本章 ∪ 分卷 ∪ 根并集。

返回中的 **`ai_guidance`** 供写章 AI 直接遵守：

1. **剧情**（`plots`）：根 / 分卷 / 本章分别按各自 order；不得混序或改写要点。  
2. **人物**（`characters`）：`linked_characters` 为本节**必须出场**的人物。  
3. **知识卡**（`knowledge`）：`linked_knowledge` 为本章**写作硬约束**（手法/文风/用词等）；优先 `extracted`。  
4. **叙事连贯**（`narrative_coherence`）：时间因果、人物一致、细节统一、段落衔接、逻辑自洽、节奏情绪、信息有效；禁止不合理、不连贯、前后冲突的叙述。  
5. **禁止项**（`forbidden`）：逻辑冲突、矛盾事实、人设崩坏、擅自加设定、机械降神、硬切场景、说明文对话、重复注水、元叙述等。  
6. **字数**（`length`）：符合 `get_novel_info` 的 `word_count_min`/`max`（约 ±60 字）。  
7. **输出**（`output`）：只输出 Markdown 正文，不要清单/自我评价。  
8. **正文用法**（`content_usage`）：本接口不含正文；生成禁止调 `get_chapter_content`；精修才另读旧稿。

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
| `node_id` | string | 否 | 章节（或支线）节点；工作台已选中时可省略 |

**返回样本**

```json
{
  "ai_guidance": {
    "plots": "严格按 linked_plots 的 order……",
    "characters": "linked_characters 为本章必须出场的人物……",
    "knowledge": "linked_knowledge 为本章写作硬约束……",
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
    "linked_character_ids": ["char-1"],
    "linked_side_plot_ids": ["plot-a", "plot-b"],
    "linked_knowledge_ids": ["kn-1"],
    "word_count": 2100
  },
  "linked_side_plot_ids": ["plot-a"],
  "linked_root_plot_ids": ["plot-root"],
  "linked_volume_plot_ids": ["plot-vol"],
  "volume": { "id": "vol-1", "label": "第一卷", "outline": "启程…" },
  "linked_knowledge_ids": ["kn-1", "kn-root"],
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
      "character": { "role": "女主", "personality": "…", "motto": "", "gender": "女", "style": "", "alignment": "" }
    }
  ],
  "linked_knowledge": [
    {
      "order": 0,
      "id": "kn-1",
      "label": "冷硬文风",
      "outline": "",
      "extract_prompt": "短句、少形容词、禁用网络梗",
      "extracted": "叙述用短句；对话克制；「很」「非常」改为具体动作……",
      "book_ids": ["book-uuid"]
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `ai_guidance.plots` | 写章须严格按剧情卡顺序与内容 |
| `ai_guidance.characters` | 关联人物必须在本章出场 |
| `ai_guidance.knowledge` | 关联知识卡为写作手法/文风等硬约束 |
| `ai_guidance.narrative_coherence` | 叙事合理连贯：时间因果、人设一致、细节统一、段落衔接、逻辑自洽等 |
| `ai_guidance.forbidden` | 禁止逻辑冲突、矛盾事实、人设崩坏、擅自加设定、机械降神、元叙述等 |
| `ai_guidance.length` | 字数符合全书每章目标（约 ±60） |
| `ai_guidance.output` | 只输出 Markdown 正文 |
| `ai_guidance.content_usage` | 本接口不含正文；生成勿调 `get_chapter_content`；精修才另读旧稿 |
| `node` | 完整 `TreeNode` |
| `linked_side_plot_ids` | 仅本章剧情 id（可排序；不含根继承） |
| `linked_root_plot_ids` | 根节点继承的剧情 id |
| `linked_plots[].order` | 根继承与本章各自从 `0` 起；看 `inherited_from_root` |
| `linked_plots[].inherited_from_root` | `true` = 根继承；`false` = 本章剧情 |
| `linked_characters` | 本章必出人物 |
| `linked_knowledge` / `linked_knowledge_ids` | 本章 + 根继承知识卡并集 |

工作台：画布把知识卡连到章节会写入 `linked_knowledge_ids`；左侧编辑栏可查看关联知识卡。预生成/精修与 MCP 写章均须严格遵守知识卡约束。

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
{ "ok": true, "word_count": 1234 }
```

| 字段 | 说明 |
|------|------|
| `word_count` | 非空白字符数（写入树节点） |

仅允许 `chapter` / `side_plot` 节点。

---

## 6. 卡片与关联

### 6.1 `upsert_character_card`

无 `node_id` 则新建（写入后自动一键排版）；有则更新。新建默认挂**章节卡**：`link_to` 优先，否则当前选中章（或选中卡的宿主），落到根且已有章节则改挂末章；无章节才挂根。边为 **章.left ← 人物.right**。显式 `link_to: root` 仍挂根。

**请求（新建）**

```json
{
  "novel_id": "n-uuid",
  "name": "林晚",
  "role": "女主",
  "gender": "女",
  "alignment": "中立善良",
  "personality": "冷静，嘴硬",
  "style": "少言，行动优先",
  "motto": "船到桥头自然直",
  "link_to": "ch-uuid"
}
```

**请求（更新）**：同上，加 `"node_id": "char-uuid"`。

**返回**：人物 `TreeNode`（含 `character` 对象）

---

### 6.2 `upsert_plot_card`

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

**返回**：剧情 `TreeNode`（`kind: "side_plot"`）

---

### 6.3 `upsert_knowledge_card`

树上知识卡（非公共库本体）。

**更新是部分字段**：省略的 `book_ids` / `extract_prompt` / `extracted` **保留原值**（便于 AI 只写回 `extracted`）。

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

**返回**：知识 `TreeNode`（`kind: "knowledge"`）。新建且未传 `link_to` 时挂当前选中章，否则末章，再否则小说根。边为 **章.left ← 知识.right**。`link_to` 为 `"root"` / `"novel"` 时解析为根节点真实 id（树里的根 id 通常是 UUID，不是 `"root"`）。

---

### 6.3.1 `fill_knowledge_card`

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

**返回**：更新后的知识 `TreeNode`

---

### 6.3.2 公共知识卡（跨小说目录）

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

### 6.4 `link_nodes`

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

### 6.5 `unlink_nodes`

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

| 改动 | 同步 |
|------|------|
| `src-tauri/src/mcp.rs` 工具名 / arguments / 返回 JSON | **本文** + 内置 skill `src-tauri/skills/novel/SKILL.md` |
| 章节 AI 预生成 / 精修 / 记忆策略 | `Project.md` |
| 应用内 Tauri `invoke` 命令 | 非本文范围（前端 `src/lib/api.ts`） |
