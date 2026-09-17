import { expect, it } from "vitest";
import { applyFontPreferences, DEFAULT_CODE_FONT, DEFAULT_UI_FONT, normalizeFontFamily, resolveFontFamilies } from "./fonts";

it("preserves default fonts for old settings and lets code follow or override the global family", () => {
  expect(resolveFontFamilies({})).toEqual({ ui: DEFAULT_UI_FONT, code: DEFAULT_CODE_FONT });
  expect(resolveFontFamilies({ fontFamily: " SimSun " }).code).toBe(`"SimSun", ${DEFAULT_CODE_FONT}`);
  expect(resolveFontFamilies({ fontFamily: "SimSun", codeFontFamily: "Consolas" }).code).toBe(`"Consolas", ${DEFAULT_CODE_FONT}`);
});
it("treats custom family names as one quoted CSS string and bounds invalid stored values", () => {
  expect(normalizeFontFamily(undefined)).toBe("");
  expect(normalizeFontFamily("bad\nfont")).toBe("");
  expect(Array.from(normalizeFontFamily("字".repeat(150)))).toHaveLength(128);
  expect(resolveFontFamilies({ fontFamily: 'font";color:red;\\name' }).ui).toBe('"font\\";color:red;\\\\name", ' + DEFAULT_UI_FONT);
});
it("applies and restores both root font tokens", () => {
  applyFontPreferences({ fontFamily: "SimSun", codeFontFamily: "Consolas" });
  expect(document.documentElement.style.getPropertyValue("--font-ui")).toContain('"SimSun"');
  expect(document.documentElement.style.getPropertyValue("--font-code")).toContain('"Consolas"');
  applyFontPreferences({ fontFamily: "", codeFontFamily: "" });
  expect(document.documentElement.style.getPropertyValue("--font-ui")).toBe(DEFAULT_UI_FONT);
});
