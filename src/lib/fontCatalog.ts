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
