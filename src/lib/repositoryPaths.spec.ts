import { describe, expect, it } from "vitest";
import { repositoryPathKey, uniqueRepositoryPaths } from "./repositoryPaths";

describe("repository path identity", () => {
  it("deduplicates Windows aliases in recent order without changing I/O paths", () => {
    const canonical = String.raw`\\?\D:\Work\Repo`;
    expect(uniqueRepositoryPaths([canonical, "d:/work/repo/", "D:\\Work\\Repo\\", "D:/other/repo", ""])).toEqual([canonical, "D:/other/repo"]);
    expect(repositoryPathKey(String.raw`\\?\UNC\server\share\repo`)).toBe(repositoryPathKey("//SERVER/share/repo/"));
    expect(repositoryPathKey("C:/")).toBe(repositoryPathKey("\\\\?\\C:\\"));
  });
  it("keeps case-sensitive Unix directories and different locations separate", () => {
    expect(uniqueRepositoryPaths(["/work/Repo", "/work/repo", "/work/repo/", "/other/repo"])).toEqual(["/work/Repo", "/work/repo", "/other/repo"]);
  });
});
