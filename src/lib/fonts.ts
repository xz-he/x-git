export interface FontPreferences { fontFamily: string; codeFontFamily: string }
export const DEFAULT_UI_FONT = 'Inter, "Microsoft YaHei", "Segoe UI", sans-serif';
export const DEFAULT_CODE_FONT = '"Cascadia Code", Consolas, "Microsoft YaHei", monospace';
export const FONT_NAME_LIMIT = 128;

// Settings represent a single installed family name, never a raw CSS declaration.
export function normalizeFontFamily(value: unknown): string {
  if (typeof value !== "string" || /[\u0000-\u001f\u007f-\u009f]/.test(value)) return "";
  return Array.from(value.trim()).slice(0, FONT_NAME_LIMIT).join("");
}
function withFallback(name: string, fallback: string): string {
  return name ? `"${name.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}", ${fallback}` : fallback;
}
export function resolveFontFamilies(preferences: Partial<FontPreferences>): { ui: string; code: string } {
  const global = normalizeFontFamily(preferences.fontFamily);
  const code = normalizeFontFamily(preferences.codeFontFamily) || global;
  return { ui: withFallback(global, DEFAULT_UI_FONT), code: withFallback(code, DEFAULT_CODE_FONT) };
}
export function applyFontPreferences(preferences: FontPreferences): void {
  const fonts = resolveFontFamilies(preferences);
  document.documentElement.style.setProperty("--font-ui", fonts.ui);
  document.documentElement.style.setProperty("--font-code", fonts.code);
}
