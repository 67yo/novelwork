import { ref } from "vue";
import { api, type SettingsView } from "@/lib/api";

export type ChatModelSetting = "chat_model" | "create_model" | "knowledge_model";

export function catalogModelIds(s: SettingsView): string[] {
  const ids = new Set<string>();
  for (const p of s.compat_providers || []) {
    for (const m of p.models || []) ids.add(m);
  }
  const c = s.model_catalog;
  if (c) {
    for (const list of [c.compat, c.gemini, c.claude, c.deepseek, c.grok, c.kimi]) {
      for (const m of list || []) ids.add(m);
    }
  }
  return [...ids].sort();
}

export function modelSelectOptions(
  s: SettingsView,
  selected: string,
  deprecatedSuffix = "",
): { id: string; label: string }[] {
  const live = new Set(catalogModelIds(s));
  const rows = [...live].sort().map((id) => ({ id, label: id }));
  if (selected && !live.has(selected)) {
    rows.unshift({ id: selected, label: `${selected}${deprecatedSuffix}` });
  }
  return rows;
}

/** Chat 面板模型：加载设置默认值，变更时写回对应任务模型。 */
export function usePersistedChatModel(setting: ChatModelSetting) {
  const model = ref("");
  const options = ref<{ id: string; label: string }[]>([]);

  async function load(deprecatedSuffix = "") {
    const s = await api.getSettings();
    model.value = (s[setting] || s.default_model || "deepseek-v4-flash").trim();
    options.value = modelSelectOptions(s, model.value, deprecatedSuffix);
  }

  async function persist() {
    const id = model.value.trim();
    if (!id) return;
    await api.saveSettings({ [setting]: id });
  }

  return { model, options, load, persist };
}
