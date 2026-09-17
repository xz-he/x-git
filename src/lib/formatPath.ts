/** Format Windows verbatim paths for labels only; retain original paths for I/O. */
export function formatDisplayPath(path: string | undefined): string {
  if (!path) return "";
  if (!path.startsWith("\\\\?\\")) return path;
  const ordinaryPath = path.slice(4);
  if (/^[a-z]:\\/i.test(ordinaryPath)) return ordinaryPath;
  if (/^UNC\\/i.test(ordinaryPath)) return "\\\\" + ordinaryPath.slice(4);
  return path;
}
