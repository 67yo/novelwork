---
name: novel
description: Novel Work 小说助手。/novel 后用 MCP 读写全书、章节、卡片与公共知识库。
trigger: true
---

你在 Novel Work 工作。读写必须走 MCP，禁止臆造 id 或库里没有的数据。工具 schema 以 MCP `tools/list` 为准。写章/改卡遵守工具返回的 `ai_guidance`。应用内 Chat 按意图裁剪工具（写章 4 个；改卡/细纲各自一小撮；分镜/公共库/世界观生成默认不暴露）；要全部工具请显式 `/novel`。

**id：** `novel_id` = 正在写的书；`book_id` / `book_ids` = Library 公共知识库。卡片（人物/剧情/知识）都在该书树上，不是公共库。已绑定则优先该书；工作台已选中时可省略 `novel_id` / `node_id`。`node_id` 也可写「第N章」、章号或唯一标题（不必先 `get_tree`）。不明则 `list_novels`。
当前选中卡片 → `get_selected_card`（默认不含正文）。列人物 → `get_character_card` 不带 `node_id`（只 id/label）；读完整卡必须带 `node_id`。upsert 只回 `ok/node_id/label`，不要把整卡再贴回对话。

## 何时用哪个工具

了解全书 → `get_novel_info`（含 `novel.features` + 根上人物/剧情/知识卡 + `volumes`）。不要一上来 `get_tree`。公共库不是小说设定源。
写/改某一章 → `get_chapter_write_context`（本会话已有根则 `include_root=false`，同卷已有则 `include_volume=false`；沿用当前 session）。遵守返回的 `ai_guidance`。不要叠 `get_novel_info`+`get_chapter_info`。细纲空则先 `generate_detailed_outline`（条数跟每章字数走）。只读正文 → `get_chapter_content`。落盘 → `set_chapter_content`（`in_band` 为真则不要再为字数改）。落盘前对照根层世界观/故事规则：冲突则就地修订，无冲突勿改。
分镜头 / MiniMax 视频：`split_chapter_shots` / `generate_shot_comfy_prompts` 只返回材料（`saved: false`），再 `set_chapter_shots` 覆盖。提交 → `submit_chapter_shots_comfyui`。只读 → `get_chapter_shots`。
章节记忆 → `get_chapter_memory` / `list_chapter_memory`；手动改 → `set_chapter_memory`；按正文抽取 → `regenerate_chapter_memory`。
改章标题/简纲/细纲 → `update_chapter_outline`。整份细纲 → `generate_detailed_outline`（材料由工具组装，须遵守世界观/故事规则/功能选项，不要先 `get_tree`/`get_novel_info`）；单条 → `regenerate_detailed_outline_item`。
补分卷 → `add_volume`（结构化 `volume`，勿写 `outline`）。读/改 → `get_volume` / `upsert_volume`。
补章节卡 → `add_chapter`。
全树/找节点 id → `get_tree`（多数情况不必：`node_id` 可直接传「第N章」/标题）。画布整列 → `layout_tree`。
建书/改书 → `create_novel` / `update_novel`。列书 → `list_novels`。
世界观 → `get_worldview`；缺卡先 `ensure_worldview`；生成 `generate_worldview`；写入 `apply_worldview`。故事规则 → `get_story_rules` / `apply_story_rules` / `generate_story_rules`。
人物卡 → `get_character_card`（指定 `node_id`）/ `upsert_character_card`（无 `personality`；回执不含整卡）。
剧情卡 → `upsert_plot_card`（回执不含整卡）。
知识卡：完整导入 `fill_knowledge_card`（回执只有字数）；AI 提炼 `search_knowledge` 最多一轮再 `upsert_knowledge_card`。根上 `slot=write_prompts`（生成/精修）及其左右子卡不进 `linked_knowledge`；Chat 写章由应用按侧别接到用户提示后（话含「精修」用右侧，否则生成用左侧）。
公共知识卡：`list_public_knowledge_cards` / `upsert_public_knowledge_card` / `archive_public_knowledge_card` / `add_public_knowledge_card`。
连线 → `link_nodes`（根上固定槽：世界大纲 `wv`、故事规则 `sr`、生成/精修 `wp`；卷/章仍 left/right/top/bottom）。拆线 → `unlink_nodes`（不可拆世界观/故事规则，也不可拆根↔生成/精修卡）。删卡 → `delete_node`（不可删生成/精修枢纽卡）。
公共库：`list_knowledge` → `search_knowledge`；入库 `import_knowledge`；归档 `archive_knowledge`；永久删 `delete_knowledge`。

改 MCP 工具或树上载荷时同步本文、`API.md` 与 `.cursor/rules/mcp-sync.mdc`。
