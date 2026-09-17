import { describe, expect, it } from "vitest";
import { formatDisplayPath } from "@/lib/formatPath";

describe("display paths", () => {
  it.each([
    [String.raw`\\?\D:\hq-project\中文 目录`, String.raw`D:\hq-project\中文 目录`],
    ["\\\\?\\C:\\", "C:\\"],
    [String.raw`\\?\UNC\server\share\repo`, String.raw`\\server\share\repo`],
    [String.raw`D:\work\repo`, String.raw`D:\work\repo`],
    [String.raw`\\server\share\repo`, String.raw`\\server\share\repo`],
    ["D:/work/repo", "D:/work/repo"],
    ["/work/repo", "/work/repo"],
    [String.raw`\\?\Volume{123}\repo`, String.raw`\\?\Volume{123}\repo`],
    [undefined, ""],
  ])("formats %s without changing other path kinds", (path, expected) => {
    expect(formatDisplayPath(path)).toBe(expected);
  });
});
