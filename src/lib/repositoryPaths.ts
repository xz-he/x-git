import { formatDisplayPath } from "./formatPath";

export const MAX_RECENT_REPOSITORIES = 20;

/** Comparison only: keep the original path for filesystem access. */
export function repositoryPathKey(path: string): string {
  const normalized = formatDisplayPath(path).replace(/\\/g, "/");
  const key = normalized.replace(/\/+$/, "") || "/";
  return /^[a-z]:\//i.test(normalized) || normalized.startsWith("//") ? key.toLowerCase() : key;
}

export function uniqueRepositoryPaths(paths: readonly string[]): string[] {
  const seen = new Set<string>();
  return paths.filter(path => {
    if (!path.trim()) return false;
    const key = repositoryPathKey(path);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
