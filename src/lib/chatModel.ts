import { ref } from "vue";
import { api, type SettingsView } from "@/lib/api";

export type ChatModelSetting = "chat_model" | "create_model" | "knowledge_model";

export function compatModelRef(providerId: string, modelId: string): string {
  return `compat:${providerId}:${modelId}`;
}

export function geminiModelRef(modelId: string): string {
  return `gemini:${modelId}`;
}

export function claudeModelRef(modelId: string): string {
  return `claude:${modelId}`;
}

/** 当前有 Key 的「提供商 × 模型」选型 id（可含同名）。 */
export function catalogModelIds(s: SettingsView): string[] {
  return modelSelectOptions(s, "", "").map((r) => r.id);
}

export function modelSelectOptions(
  s: SettingsView,
  selected: string,
  deprecatedSuffix = "",
): { id: string; label: string }[] {
  const rows: { id: string; label: string }[] = [];
  const seen = new Set<string>();

  for (const p of s.compat_providers || []) {
    if (!p.api_key_configured) continue;
    const label = (p.label || "").trim() || p.id.slice(0, 8) || "OpenAI";
    for (const m of p.models || []) {
      if (!m) continue;
      const id = compatModelRef(p.id, m);
      if (seen.has(id)) continue;
      seen.add(id);
      rows.push({ id, label: `${label} · ${m}` });
    }
  }

  const c = s.model_catalog;
  if (c && s.gemini_api_key_configured) {
    for (const m of c.gemini || []) {
      if (!m) continue;
      const id = geminiModelRef(m);
      if (seen.has(id)) continue;
      seen.add(id);
      rows.push({ id, label: `Gemini · ${m}` });
    }
  }
  if (c && s.claude_api_key_configured) {
    for (const m of c.claude || []) {
      if (!m) continue;
      const id = claudeModelRef(m);
      if (seen.has(id)) continue;
      seen.add(id);
      rows.push({ id, label: `Claude · ${m}` });
    }
  }

  rows.sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }));

  // 仅在仍有可用模型时保留「已下架」选中项
  if (selected && !seen.has(selected) && rows.length > 0) {
    rows.unshift({ id: selected, label: `${selected}${deprecatedSuffix}` });
  }
  return rows;
}

/** 旧裸名 → 第一个含该模型的 qualified ref；已是 ref 则原样返回（若仍在列表中）。 */
export function resolvePreferredModelId(s: SettingsView, preferred: string): string {
  const live = catalogModelIds(s);
  if (live.length === 0) return "";
  const pref = preferred.trim();
  if (pref && live.includes(pref)) return pref;
  if (pref) {
    const byBare = live.find((id) => {
      if (id.startsWith("compat:")) {
        const rest = id.slice("compat:".length);
        const i = rest.indexOf(":");
        return i >= 0 && rest.slice(i + 1) === pref;
      }
      if (id.startsWith("gemini:")) return id.slice("gemini:".length) === pref;
      if (id.startsWith("claude:")) return id.slice("claude:".length) === pref;
      return id === pref;
    });
    if (byBare) return byBare;
  }
  return live[0];
}

/** Chat 面板模型：加载设置默认值，变更时写回 chat_model（全应用 LLM 共用）。 */
export function usePersistedChatModel(setting: ChatModelSetting) {
  const model = ref("");
  const options = ref<{ id: string; label: string }[]>([]);

  async function load(deprecatedSuffix = "") {
    const s = await api.getSettings();
    const live = catalogModelIds(s);
    if (live.length === 0) {
      model.value = "";
      options.value = [];
      return;
    }
    const preferred = (s[setting] || s.default_model || "").trim();
    model.value = resolvePreferredModelId(s, preferred);
    options.value = modelSelectOptions(s, model.value, deprecatedSuffix);
  }

  async function persist() {
    const id = model.value.trim();
    if (!id) return;
    await api.saveSettings({ [setting]: id });
  }

  return { model, options, load, persist };
}
