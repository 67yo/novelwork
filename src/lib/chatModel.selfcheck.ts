import {
  catalogModelIds,
  compatModelRef,
  modelSelectOptions,
  resolvePreferredModelId,
} from "./chatModel";
import type { SettingsView } from "./api";

function emptySettings(over: Partial<SettingsView> = {}): SettingsView {
  return {
    compat_providers: [],
    default_model: "deepseek-chat",
    create_model: "deepseek-v4-flash",
    generate_model: "deepseek-v4-flash",
    chat_model: "deepseek-v4-flash",
    refine_model: "deepseek-reasoner",
    knowledge_model: "deepseek-v4-flash",
    model_catalog: {
      deepseek: ["stale-deepseek"],
      gemini: ["gemini-pro"],
      claude: ["claude-3"],
      grok: ["grok-1"],
      kimi: ["kimi-1"],
      compat: ["stale-compat"],
      updated_at: "",
      errors: {},
    },
    ui_locale: "system",
    mcp_port: 17832,
    mcp_enabled: true,
    mcp_lan: false,
    ai_interaction_log: false,
    ai_log_dir: "",
    comfyui_url: "http://127.0.0.1:8188",
    comfyui_workflow: "",
    comfyui_prompt_node: "",
    comfyui_image_workflow: "",
    ...over,
  };
}

const none = emptySettings();
console.assert(catalogModelIds(none).length === 0, "no keys → no catalog ids");
console.assert(
  modelSelectOptions(none, "deepseek-v4-flash", " (旧)").length === 0,
  "no keys → no select options even with saved chat_model",
);

const withCompat = emptySettings({
  compat_providers: [
    {
      id: "1",
      label: "DS",
      protocol: "openai",
      base_url: "https://api.deepseek.com",
      api_key_masked: "sk-****",
      api_key_configured: true,
      models: ["deepseek-chat"],
    },
  ],
});
console.assert(
  JSON.stringify(catalogModelIds(withCompat)) === JSON.stringify(["compat:1:deepseek-chat"]),
  "compat qualified id",
);
const opts = modelSelectOptions(withCompat, "gone-model", " (旧)");
console.assert(opts[0]?.id === "gone-model" && opts.length === 2, "deprecated only when live nonempty");
console.assert(opts[1]?.label === "DS · deepseek-chat", "label shows provider");

const dup = emptySettings({
  compat_providers: [
    {
      id: "a",
      label: "Alpha",
      protocol: "openai",
      base_url: "https://a.example",
      api_key_masked: "a",
      api_key_configured: true,
      models: ["gpt-4"],
    },
    {
      id: "b",
      label: "Beta",
      protocol: "openai",
      base_url: "https://b.example",
      api_key_masked: "b",
      api_key_configured: true,
      models: ["gpt-4"],
    },
  ],
});
const dupIds = catalogModelIds(dup);
console.assert(dupIds.length === 2, "same model name → two options");
console.assert(dupIds.includes(compatModelRef("a", "gpt-4")), "includes a");
console.assert(dupIds.includes(compatModelRef("b", "gpt-4")), "includes b");
console.assert(
  resolvePreferredModelId(dup, "compat:b:gpt-4") === "compat:b:gpt-4",
  "keep qualified",
);
console.assert(
  resolvePreferredModelId(dup, "gpt-4") === "compat:a:gpt-4",
  "bare name → first provider",
);

const gem = emptySettings({
  compat_providers: [
    {
      id: "g",
      label: "Gemini",
      protocol: "gemini",
      base_url: "https://generativelanguage.googleapis.com",
      api_key_masked: "AIza",
      api_key_configured: true,
      models: ["gemini-pro"],
    },
  ],
});
console.assert(
  resolvePreferredModelId(gem, "gemini:gemini-pro") === "compat:g:gemini-pro",
  "legacy gemini: prefix → AI API provider",
);
console.assert(
  !catalogModelIds(gem).some((id) => id.startsWith("gemini:")),
  "no standalone gemini: catalog ids",
);

console.log("chatModel.selfcheck ok");
