import { invoke } from "@tauri-apps/api/core";

export type CompatProviderView = {
  id: string;
  label: string;
  protocol: string;
  base_url: string;
  api_key_masked: string;
  api_key_configured: boolean;
  models: string[];
};

export type ModelCatalog = {
  deepseek: string[];
  gemini: string[];
  claude: string[];
  grok: string[];
  kimi: string[];
  compat: string[];
  updated_at: string;
  errors: Record<string, string>;
};

export type SettingsView = {
  compat_providers: CompatProviderView[];
  gemini_api_key_masked: string;
  gemini_api_key_configured: boolean;
  claude_api_key_masked: string;
  claude_api_key_configured: boolean;
  default_model: string;
  create_model: string;
  generate_model: string;
  chat_model: string;
  refine_model: string;
  knowledge_model: string;
  model_catalog: ModelCatalog;
  ui_locale: string;
  mcp_port: number;
  mcp_enabled: boolean;
  mcp_lan: boolean;
};

export type McpStatus = {
  running: boolean;
  port: number;
  endpoint: string;
  lan_enabled: boolean;
  lan_endpoint: string | null;
  error: string | null;
};

export type SkillSlashHint = {
  cmd: string;
  hint: string;
};

export type SkillPreviewItem = {
  name: string;
  description: string;
  path: string;
  trigger: boolean;
  tags: string[];
  hint: string | null;
  body_preview: string;
  builtin?: boolean;
  slash_hints?: SkillSlashHint[];
};

export type SkillsPreview = {
  root: string;
  bundled?: string;
  user?: string;
  exists: boolean;
  skills: SkillPreviewItem[];
};

export type SkillMatchPreview = {
  matched: boolean;
  name: string | null;
  score: number | null;
  via: string;
};

export type KnowledgeBook = {
  id: string;
  title: string;
  author: string;
  genres: string[];
  source_path: string;
  extract_prompt: string;
  created_at: string;
  chunk_count: number;
  archived: boolean;
};

export type KnowledgeChunk = {
  idx: number;
  content: string;
};

export type NovelProject = {
  id: string;
  title: string;
  synopsis: string;
  cover_path: string | null;
  knowledge_ids: string[];
  knowledge_strategy: string;
  /** reference | strict */
  canon_mode: string;
  archived: boolean;
  word_count_min: number;
  word_count_max: number;
  chapter_count: number;
  created_at: string;
  updated_at: string;
};

export type CharacterCard = {
  role: string;
  personality: string;
  motto: string;
  gender: string;
  style: string;
  alignment: string;
};

export type KnowledgeCardPayload = {
  book_ids: string[];
  extract_prompt: string;
  extracted: string;
  from_canon?: boolean;
};

export type PublicKnowledgeCard = {
  id: string;
  title: string;
  book_ids: string[];
  extract_prompt: string;
  extracted: string;
  created_at: string;
  updated_at: string;
  archived: boolean;
};

export type SidePlotMeta = {
  /** active | resolved | deferred */
  status: string;
  absorbed: boolean;
};

export type TreeNode = {
  id: string;
  kind: "novel" | "volume" | "chapter" | "character" | "side_plot" | "knowledge";
  label: string;
  outline: string;
  character: CharacterCard | null;
  knowledge: KnowledgeCardPayload | null;
  side_plot?: SidePlotMeta | null;
  linked_character_ids: string[];
  linked_side_plot_ids: string[];
  linked_knowledge_ids: string[];
  position: { x: number; y: number };
  word_count: number;
  word_count_min: number;
  word_count_max: number;
  chapter_count: number;
};

export type TreeEdge = {
  id: string;
  source: string;
  target: string;
  kind: string;
  source_handle?: string | null;
  target_handle?: string | null;
  /** 人物↔人物关系说明 */
  label?: string;
};

export type NovelTree = {
  novel_id: string;
  nodes: TreeNode[];
  edges: TreeEdge[];
};

export type GlobalChatMessage = {
  id: string;
  role: string;
  content: string;
  created_at: string;
};

export type GenerateResult = {
  content: string;
  used_mock: boolean;
  message: string;
};

export type ChapterMemoryGroup = {
  node_id: string;
  label: string;
  items: string[];
};

export type TokenUsageHourRow = {
  hour: number;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
};

export type TokenUsageNovelRow = {
  novel_id: string;
  title: string;
  day_prompt_tokens: number;
  day_completion_tokens: number;
  day_tokens: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  total_tokens: number;
};

export type TokenUsageDay = {
  date: string;
  rows: TokenUsageHourRow[];
  models: string[];
  novels: TokenUsageNovelRow[];
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
};

export type TokenUsageModelRow = {
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
};

export type TokenUsageMonth = {
  month: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  models: TokenUsageModelRow[];
};

export type ChatTurn = {
  role: string;
  content: string;
};

export type NovelCreateChatResult = {
  reply: string;
  used_mock: boolean;
  novel: NovelProject | null;
};

/** 知识卡：把所选公共库正文完整导入到 extracted */
export function fillKnowledgeCard(novelId: string, nodeId: string) {
  return invoke<NovelTree>("fill_knowledge_card", { novelId, nodeId });
}

export const api = {
  getSettings: () => invoke<SettingsView>("get_settings"),
  refreshModelCatalog: () => invoke<ModelCatalog>("refresh_model_catalog"),
  fetchCompatModels: (baseUrl: string, apiKey: string) =>
    invoke<string[]>("fetch_compat_models", { baseUrl, apiKey }),
  refreshCompatProviderModels: (providerId: string) =>
    invoke<string[]>("refresh_compat_provider_models", { providerId }),
  saveSettings: (input: {
    compat_providers?: {
      id: string;
      label: string;
      protocol: string;
      base_url: string;
      api_key?: string | null;
      models: string[];
    }[];
    gemini_api_key?: string | null;
    claude_api_key?: string | null;
    default_model?: string | null;
    create_model?: string | null;
    generate_model?: string | null;
    chat_model?: string | null;
    refine_model?: string | null;
    knowledge_model?: string | null;
    ui_locale?: string | null;
    mcp_port?: number | null;
    mcp_enabled?: boolean | null;
    mcp_lan?: boolean | null;
  }) =>
    invoke<SettingsView>("save_settings", {
      input: {
        compatProviders: input.compat_providers
          ? input.compat_providers.map((p) => ({
              id: p.id,
              label: p.label,
              protocol: p.protocol,
              baseUrl: p.base_url,
              apiKey: p.api_key ?? null,
              models: p.models,
            }))
          : null,
        geminiApiKey: input.gemini_api_key ?? null,
        claudeApiKey: input.claude_api_key ?? null,
        defaultModel: input.default_model ?? null,
        createModel: input.create_model ?? null,
        generateModel: input.generate_model ?? null,
        chatModel: input.chat_model ?? null,
        refineModel: input.refine_model ?? null,
        knowledgeModel: input.knowledge_model ?? null,
        uiLocale: input.ui_locale ?? null,
        mcpPort: input.mcp_port ?? null,
        mcpEnabled: input.mcp_enabled ?? null,
        mcpLan: input.mcp_lan ?? null,
      },
    }),
  getMcpStatus: () => invoke<McpStatus>("get_mcp_status"),
  restartMcpServer: () => invoke<McpStatus>("restart_mcp_server"),
  setWorkspaceSelection: (novelId: string | null, nodeId: string | null) =>
    invoke<void>("set_workspace_selection", { novelId, nodeId }),
  listGenres: () => invoke<string[]>("list_genres"),
  listKnowledge: () => invoke<KnowledgeBook[]>("list_knowledge_bases"),
  renameKnowledge: (id: string, title: string) =>
    invoke<void>("rename_knowledge", { id, title }),
  updateKnowledge: (
    id: string,
    title: string,
    author: string,
    extract_prompt: string,
    genres: string[],
  ) =>
    invoke<KnowledgeBook>("update_knowledge", {
      id,
      title,
      author,
      extractPrompt: extract_prompt,
      genres,
    }),
  listKnowledgeChunks: (bookId: string) =>
    invoke<KnowledgeChunk[]>("list_knowledge_chunks", { bookId }),
  archiveKnowledge: (id: string, archived: boolean) =>
    invoke<KnowledgeBook>("archive_knowledge", { id, archived }),
  deleteKnowledge: (id: string) => invoke<void>("delete_knowledge", { id }),
  importKnowledge: (path: string, genres: string[]) =>
    invoke<KnowledgeBook>("import_knowledge_text", { path, genres }),
  importKnowledgeUrl: (url: string, genres: string[]) =>
    invoke<KnowledgeBook>("import_knowledge_url", { url, genres }),
  reextractKnowledge: (id: string, path?: string | null) =>
    invoke<KnowledgeBook>("reextract_knowledge", {
      id,
      path: path ?? null,
    }),
  knowledgeIndexStatus: (bookId: string) =>
    invoke<{ bookId: string; chunkCount: number; embeddingCount: number }>(
      "knowledge_index_status",
      { bookId },
    ),
  rebuildKnowledgeIndex: (bookId: string) =>
    invoke<{ bookId: string; chunkCount: number; embeddingCount: number }>(
      "rebuild_knowledge_index",
      { bookId },
    ),
  listNovels: () => invoke<NovelProject[]>("list_novels"),
  getNovel: (id: string) => invoke<NovelProject | null>("get_novel", { id }),
  updateNovelPlan: (
    novelId: string,
    word_count_min: number,
    word_count_max: number,
    chapter_count: number,
  ) =>
    invoke<NovelProject>("update_novel_plan", {
      novelId,
      wordCountMin: word_count_min,
      wordCountMax: word_count_max,
      chapterCount: chapter_count,
    }),
  archiveNovel: (id: string, archived: boolean) =>
    invoke<NovelProject>("archive_novel", { id, archived }),
  deleteNovel: (id: string) => invoke<void>("delete_novel", { id }),
  createNovel: (input: {
    title: string;
    synopsis: string;
    knowledge_ids: string[];
    knowledge_strategy: string;
  }) =>
    invoke<NovelProject>("create_novel", {
      input: {
        title: input.title,
        synopsis: input.synopsis,
        knowledgeIds: input.knowledge_ids,
        knowledgeStrategy: input.knowledge_strategy,
      },
    }),
  createNovelChat: (messages: ChatTurn[], forceCreate = false, model?: string | null) =>
    invoke<NovelCreateChatResult>("create_novel_chat", {
      input: { messages, forceCreate, model: model?.trim() || null },
    }),
  getTree: (novelId: string) => invoke<NovelTree>("get_tree", { novelId }),
  saveTree: (tree: NovelTree) => invoke<void>("save_tree", { tree }),
  deleteTreeCard: (novelId: string, nodeId: string) =>
    invoke<NovelTree>("delete_tree_card", { novelId, nodeId }),
  getChapter: (novelId: string, nodeId: string) =>
    invoke<string>("get_chapter", { novelId, nodeId }),
  /** 手动保存章节正文；返回字数。空内容清空文件 */
  saveChapter: (novelId: string, nodeId: string, content: string) =>
    invoke<number>("save_chapter", { novelId, nodeId, content }),
  /** 按修改意见改写单段正文；模型同全局 Chat（chat_model） */
  rewriteChapterParagraph: (
    novelId: string,
    paragraph: string,
    instruction: string,
    model?: string | null,
  ) =>
    invoke<string>("rewrite_chapter_paragraph", {
      novelId,
      paragraph,
      instruction,
      model: model?.trim() || null,
    }),
  getChapterMemory: (novelId: string, nodeId: string) =>
    invoke<string[]>("get_chapter_memory", { novelId, nodeId }),
  listAllChapterMemory: (novelId: string) =>
    invoke<ChapterMemoryGroup[]>("list_all_chapter_memory", { novelId }),
  /** `items` 为空则清除该章全部记忆 */
  setChapterMemory: (novelId: string, nodeId: string, items: string[]) =>
    invoke<void>("set_chapter_memory", { novelId, nodeId, items }),
  regenerateChapterMemory: (
    novelId: string,
    nodeId: string,
    userNotes = "",
    memoryNodeIds: string[] = [],
  ) =>
    invoke<string[]>("regenerate_chapter_memory", {
      novelId,
      nodeId,
      userNotes,
      memoryNodeIds,
    }),
  /** 预生成确认：前序章列表（是否已有记忆） */
  previewGenerateChapter: (novelId: string, nodeId: string) =>
    invoke<{ node_id: string; label: string; has_memory: boolean }[]>(
      "preview_generate_chapter",
      { novelId, nodeId },
    ),
  generateChapter: (
    novelId: string,
    nodeId: string,
    memoryNodeIds: string[],
    userBrief: string,
    model?: string | null,
  ) =>
    invoke<GenerateResult>("generate_chapter", {
      novelId,
      nodeId,
      memoryNodeIds,
      userBrief,
      model: model?.trim() || null,
    }),
  refineChapter: (
    novelId: string,
    nodeId: string,
    prevN: number,
    mode?: "memory" | "full",
    userBrief?: string,
    model?: string | null,
  ) =>
    invoke<GenerateResult>("refine_chapter", {
      novelId,
      nodeId,
      prevN,
      mode: mode ?? "memory",
      userBrief: userBrief ?? "",
      model: model?.trim() || null,
    }),
  /** 左侧生成确认框：根大纲→前序大纲+记忆 + 期望占位 */
  previewChapterOutlineBrief: (
    novelId: string,
    mode: "root" | "plan_next" | "regen_outline",
    count: number,
    nodeId?: string,
  ) =>
    invoke<{ brief: string; from: number; to: number }>("preview_chapter_outline_brief", {
      novelId,
      mode,
      count,
      nodeId: nodeId ?? null,
    }),
  planNextChapters: (novelId: string, nodeId: string, count: number, userBrief: string) =>
    invoke<GenerateResult>("plan_next_chapters", {
      novelId,
      nodeId,
      count,
      userBrief,
    }),
  /** 覆盖重写本章标题与大纲（Chat `/刷新本章大纲`；同 generate_chapter_cards） */
  regenerateChapterOutline: (
    novelId: string,
    nodeId: string,
    userBrief: string,
    model?: string | null,
  ) =>
    invoke<GenerateResult>("regenerate_chapter_outline", {
      novelId,
      nodeId,
      userBrief,
      model: model?.trim() || null,
    }),
  /** 章节卡：按大纲+用户补充生成剧情卡 */
  generateChapterPlots: (
    novelId: string,
    nodeId: string,
    outline: string,
    userNotes: string,
  ) =>
    invoke<GenerateResult>("generate_chapter_plots", {
      novelId,
      nodeId,
      outline,
      userNotes,
    }),
  /** 根节点：近 N 章大纲+记忆 → 整理跨章剧情卡 */
  consolidatePlotCards: (novelId: string, chapterWindow: number, userNotes: string) =>
    invoke<GenerateResult>("consolidate_plot_cards", {
      novelId,
      chapterWindow,
      userNotes,
    }),
  /** 根节点：生成第 1–count 章章节卡（同号覆盖） */
  generateChapterCards: (novelId: string, count: number, userBrief: string) =>
    invoke<GenerateResult>("generate_chapter_cards", { novelId, count, userBrief }),
  /** 终止进行中的 chat（开书 `__create__` 等） */
  chatCancel: (novelId: string) => invoke<void>("chat_cancel", { novelId }),
  fillKnowledgeCard,
  listPublicKnowledgeCards: () => invoke<PublicKnowledgeCard[]>("list_public_knowledge_cards"),
  upsertPublicKnowledgeCard: (input: {
    id?: string | null;
    title: string;
    book_ids: string[];
    extract_prompt: string;
    extracted: string;
  }) =>
    invoke<PublicKnowledgeCard>("upsert_public_knowledge_card", {
      id: input.id ?? null,
      title: input.title,
      bookIds: input.book_ids,
      extractPrompt: input.extract_prompt,
      extracted: input.extracted,
    }),
  archivePublicKnowledgeCard: (id: string, archived: boolean) =>
    invoke<PublicKnowledgeCard>("archive_public_knowledge_card", { id, archived }),
  deletePublicKnowledgeCard: (id: string) =>
    invoke<void>("delete_public_knowledge_card", { id }),
  addPublicKnowledgeCard: (novelId: string, publicId: string, hostNodeId?: string | null) =>
    invoke<NovelTree>("add_public_knowledge_card", {
      novelId,
      publicId,
      hostNodeId: hostNodeId ?? null,
    }),
  generateCoverPrompt: (novelId: string) =>
    invoke<string>("generate_cover_prompt", { novelId }),
  pickCover: () => invoke<string | null>("pick_cover"),
  setCover: (novelId: string, sourcePath: string) =>
    invoke<NovelProject>("set_cover", { novelId, sourcePath }),
  pickTextFile: () => invoke<string | null>("pick_text_file"),
  appDataRoot: () => invoke<string>("app_data_root"),
  listChatSkills: () => invoke<SkillsPreview>("list_chat_skills"),
  previewChatSkillMatch: (query: string) =>
    invoke<SkillMatchPreview>("preview_chat_skill_match", { query }),
  getTokenUsage: (date: string) => invoke<TokenUsageDay>("get_token_usage", { date }),
  getTokenUsageMonth: (month: string) =>
    invoke<TokenUsageMonth>("get_token_usage_month", { month }),
  globalChatList: (novelId?: string | null) =>
    invoke<GlobalChatMessage[]>("global_chat_list", { novelId: novelId ?? null }),
  globalChatSend: (args: {
    content: string;
    routeName?: string | null;
    path?: string | null;
    novelId?: string | null;
    model?: string | null;
  }) =>
    invoke<GlobalChatMessage[]>("global_chat_send", {
      content: args.content,
      routeName: args.routeName ?? null,
      path: args.path ?? null,
      novelId: args.novelId ?? null,
      model: args.model?.trim() || null,
    }),
  globalChatClear: (novelId?: string | null) =>
    invoke<void>("global_chat_clear", { novelId: novelId ?? null }),
  globalChatCancel: () => invoke<void>("global_chat_cancel"),
};
