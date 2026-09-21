// Fallback suggestions only; the desktop normally lists installed families.
export const FALLBACK_FONT_FAMILIES = [
  "Microsoft YaHei", "Microsoft YaHei UI", "DengXian", "SimSun", "NSimSun", "SimHei", "KaiTi", "FangSong",
  "STSong", "STHeiti", "STKaiti", "STFangsong", "STXihei", "STZhongsong", "STXingkai", "STLiti", "STHupo", "STXinwei",
  "Source Han Sans SC", "Source Han Serif SC", "Noto Sans CJK SC", "Noto Serif CJK SC", "HarmonyOS Sans SC", "MiSans", "LXGW WenKai",
  "Segoe UI", "Segoe UI Variable", "Arial", "Calibri", "Cambria", "Candara", "Tahoma", "Verdana", "Trebuchet MS", "Times New Roman", "Georgia",
  "Consolas", "Cascadia Code", "Cascadia Mono", "JetBrains Mono", "Fira Code", "Source Code Pro", "Ubuntu Mono", "Courier New", "Lucida Console", "IBM Plex Mono",
];

const CHINESE_FONT_NAMES: Record<string, string> = {
  "Microsoft YaHei": "微软雅黑", "Microsoft YaHei UI": "微软雅黑 UI", DengXian: "等线",
  SimSun: "宋体", NSimSun: "新宋体", SimHei: "黑体", KaiTi: "楷体", FangSong: "仿宋",
  STSong: "华文宋体", STHeiti: "华文黑体", STKaiti: "华文楷体", STFangsong: "华文仿宋", STXihei: "华文细黑",
  STZhongsong: "华文中宋", STXingkai: "华文行楷", STLiti: "华文隶书", STHupo: "华文琥珀", STXinwei: "华文新魏",
  "Source Han Sans SC": "思源黑体", "Source Han Serif SC": "思源宋体", "Noto Sans CJK SC": "思源黑体",
  "Noto Serif CJK SC": "思源宋体", "HarmonyOS Sans SC": "鸿蒙黑体", "LXGW WenKai": "霞鹜文楷",
};

export function fontOptions(families: readonly string[]): { value: string; label: string }[] {
  return [...new Set(families)].map(value => ({ value, label: CHINESE_FONT_NAMES[value] ? `${CHINESE_FONT_NAMES[value]} · ${value}` : value }))
    .sort((left, right) => left.label.localeCompare(right.label, "zh-CN", { numeric: true }));
}

const CLEAR_UI_FAMILIES = ["Microsoft YaHei UI", "Microsoft YaHei", "微软雅黑 UI", "微软雅黑", "Noto Sans CJK SC", "Source Han Sans SC", "思源黑体", "Segoe UI"];
const CLEAR_CODE_FAMILIES = ["Cascadia Mono", "Consolas", "Cascadia Code", "JetBrains Mono"];
const SMALL_TEXT_FONTS = /^(?:楷体(?:_GB2312)?|仿宋(?:_GB2312)?|宋体|新宋体|华文(?:楷体|仿宋|行楷|细黑)|KaiTi(?:_GB2312)?|FangSong(?:_GB2312)?|[N]?SimSun|STKaiti|STFangsong|STXingkai|STXihei)$|(?:\s|^)(?:Light|Thin|ExtraLight|UltraLight)$/i;

export function hasFineStrokes(family: string): boolean { return SMALL_TEXT_FONTS.test(family.trim()); }

export function clearFontPreferences(families: readonly string[]): { fontFamily: string; codeFontFamily: string } {
  const pick = (preferred: string[]) => preferred.map(name => families.find(family => family.toLowerCase() === name.toLowerCase())).find(Boolean) ?? "";
  // Empty values keep the app's screen-font fallbacks when enumeration fails.
  return { fontFamily: pick(CLEAR_UI_FAMILIES), codeFontFamily: pick(CLEAR_CODE_FAMILIES) };
}
