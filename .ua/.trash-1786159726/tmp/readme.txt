# Nove Work

强约束 AI 小说写作工具（Demo）。域名：[nove.work](https://nove.work) / novework.net

## 技术栈

- Tauri 2 + Rust
- Vite + Vue 3 + TypeScript + Tailwind + shadcn-vue 风格组件
- SQLite（项目 / Chat / 设置）
- LanceDB（知识库分段）
- DeepSeek API（`rig-core` / `adk-rust` 已接入链路）

## 开发

```bash
pnpm install
pnpm tauri dev
```

需要本机已安装 Rust、pnpm。macOS / Windows。

## DeepSeek API Key

1. 启动应用后打开 **设置**
2. 填写 DeepSeek API Key（可选改 Base URL，默认 `https://api.deepseek.com`）
3. 默认模型 `deepseek-chat`
4. 未配置 Key 时，预生成 / 精修 / Chat 会走 Mock，并提示去设置页填写

密钥经 **AES-256-GCM** 加密后保存在本地 SQLite（派生密钥绑定本机 hostname + 用户名），不会写进仓库。旧版明文 Key 会在下次读取时自动迁移为密文。

## Demo 功能

1. **知识库**：导入 txt，按约 1000 字分段、20 字重叠；EPUB/PDF 入口预留
2. **小说列表**：SQLite 持久化；自带示例小说「雾港回声」
3. **工作台**：左侧可缩放节点树 · 中间章节预览（预生成 / 精修）· 右侧 Chat

数据目录：

```text
novework/
  nove.db
  lancedb/
  novels/{id}/meta.json, tree.json, chapters/*.md, cover.*
```
