import { invoke } from "@tauri-apps/api/core";

export type ModelCatalog = {
  deepseek: string[];
  gemini: string[];
  claude: string[];
  grok: string[];
  updated_at: string;
  errors: Record<string, string>;
};

export type SettingsView = {
  deepseek_api_key_masked: string;
  deepseek_api_key_configured: boolean;
  chatgpt_api_key_masked: string;
  chatgpt_api_key_configured: boolean;
  gemini_api_key_masked: string;
  gemini_api_key_configured: boolean;
  claude_api_key_masked: string;
  claude_api_key_configured: boolean;
  grok_api_key_masked: string;
  grok_api_key_configured: boolean;
  deepseek_base_url: string;
  default_model: string;
  create_model: string;
  generate_model: string;
  chat_model: string;
  refine_model: string;
  model_catalog: ModelCatalog;
  ui_locale: string;
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
};

export type NovelProject = {
  id: string;
  title: string;
  synopsis: string;
  cover_path: string | null;
  knowledge_ids: string[];
  knowledge_strategy: string;
  archived: boolean;
  word_count_min: number;
  word_count_max: number;
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

export type TreeNode = {
  id: string;
  kind: "novel" | "chapter" | "character" | "side_plot";
  label: string;
  outline: string;
  character: CharacterCard | null;
  linked_character_ids: string[];
  linked_side_plot_ids: string[];
  position: { x: number; y: number };
  word_count: number;
  word_count_min: number;
  word_count_max: number;
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

export type ChatMessage = {
  id: string;
  novel_id: string;
  role: string;
  content: string;
  created_at: string;
};

export type GenerateResult = {
  content: string;
  used_mock: boolean;
  message: string;
};

export type CardChatResult = {
  reply: string;
  tree: NovelTree;
};

export type TokenUsageHourRow = {
  hour: number;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
};

export type TokenUsageDay = {
  date: string;
  rows: TokenUsageHourRow[];
  models: string[];
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

export const api = {
  getSettings: () => invoke<SettingsView>("get_settings"),
  refreshModelCatalog: () => invoke<ModelCatalog>("refresh_model_catalog"),
  saveSettings: (input: {
    deepseek_api_key?: string | null;
    chatgpt_api_key?: string | null;
    gemini_api_key?: string | null;
    claude_api_key?: string | null;
    grok_api_key?: string | null;
    deepseek_base_url?: string | null;
    default_model?: string | null;
    create_model?: string | null;
    generate_model?: string | null;
    chat_model?: string | null;
    refine_model?: string | null;
    ui_locale?: string | null;
  }) =>
    invoke<SettingsView>("save_settings", {
      input: {
        deepseekApiKey: input.deepseek_api_key ?? null,
        chatgptApiKey: input.chatgpt_api_key ?? null,
        geminiApiKey: input.gemini_api_key ?? null,
        claudeApiKey: input.claude_api_key ?? null,
        grokApiKey: input.grok_api_key ?? null,
        deepseekBaseUrl: input.deepseek_base_url ?? null,
        defaultModel: input.default_model ?? null,
        createModel: input.create_model ?? null,
        generateModel: input.generate_model ?? null,
        chatModel: input.chat_model ?? null,
        refineModel: input.refine_model ?? null,
        uiLocale: input.ui_locale ?? null,
      },
    }),
  listGenres: () => invoke<string[]>("list_genres"),
  listKnowledge: () => invoke<KnowledgeBook[]>("list_knowledge_bases"),
  importKnowledge: (path: string, extract_prompt: string, genres: string[]) =>
    invoke<KnowledgeBook>("import_knowledge_text", { path, extractPrompt: extract_prompt, genres }),
  listNovels: () => invoke<NovelProject[]>("list_novels"),
  getNovel: (id: string) => invoke<NovelProject | null>("get_novel", { id }),
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
  createNovelChat: (messages: ChatTurn[], forceCreate = false) =>
    invoke<NovelCreateChatResult>("create_novel_chat", {
      input: { messages, forceCreate },
    }),
  getTree: (novelId: string) => invoke<NovelTree>("get_tree", { novelId }),
  saveTree: (tree: NovelTree) => invoke<void>("save_tree", { tree }),
  getChapter: (novelId: string, nodeId: string) =>
    invoke<string>("get_chapter", { novelId, nodeId }),
  generateChapter: (novelId: string, nodeId: string) =>
    invoke<GenerateResult>("generate_chapter", { novelId, nodeId }),
  refineChapter: (novelId: string, nodeId: string, prevN: number) =>
    invoke<GenerateResult>("refine_chapter", { novelId, nodeId, prevN }),
  listChat: (novelId: string) => invoke<ChatMessage[]>("list_chat_messages", { novelId }),
  chatSend: (novelId: string, content: string) =>
    invoke<ChatMessage[]>("chat_send", { novelId, content }),
  cardChatSend: (novelId: string, nodeId: string, content: string) =>
    invoke<CardChatResult>("card_chat_send", { novelId, nodeId, content }),
  pickCover: () => invoke<string | null>("pick_cover"),
  setCover: (novelId: string, sourcePath: string) =>
    invoke<NovelProject>("set_cover", { novelId, sourcePath }),
  pickTextFile: () => invoke<string | null>("pick_text_file"),
  appDataRoot: () => invoke<string>("app_data_root"),
  getTokenUsage: (date: string) => invoke<TokenUsageDay>("get_token_usage", { date }),
};
