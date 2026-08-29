# Novel Work

强约束 AI 小说写作工具（Demo）。介绍站：[novelwork.net](https://novelwork.net)

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

## 打包

### Windows 安装包（NSIS，`*-setup.exe`）

在 **Windows** 上（需 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) + WebView2 运行时环境）：

```bash
pnpm install
pnpm build:windows
```

安装包输出目录：

```text
src-tauri/target/release/bundle/nsis/
```

也可在 GitHub Actions 手动触发 **windows-nsis** workflow，或推送 `v*` 标签；产物为 Artifact `Novel-Work-windows-nsis`。

配置见 `src-tauri/tauri.conf.json`（`bundle.windows.nsis`）与 `tauri.windows.conf.json`。当前为当前用户安装、简中/英文可选；未配置代码签名。

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
