import { ref } from "vue";
import { messages } from "./locales/messages";

export type AppLanguage = "zh-CN" | "en" | "bilingual";
export const language = ref<AppLanguage>("zh-CN");
export function normalizeLanguage(value: unknown): AppLanguage {
  return value === "en" || value === "bilingual" ? value : "zh-CN";
}
export function applyLanguage(value: unknown): void {
  language.value = normalizeLanguage(value);
  document.documentElement.lang = language.value === "en" ? "en" : "zh-CN";
  document.documentElement.dataset.language = language.value;
}

export function t(key: keyof typeof messages, params: Record<string, unknown> = {}, locale: AppLanguage = language.value): string {
  const message: { zh: string; en: string } = messages[key];
  const format = (template: string) => template.replace(/\{(\w+)\}/g, (match, name: string) => Object.hasOwn(params, name) ? String(params[name] ?? "") : match);
  if (locale === "zh-CN") return format(message.zh);
  if (locale === "en" || message.zh === message.en) return format(message.en);
  return `${format(message.en)} · ${format(message.zh)}`;
}
