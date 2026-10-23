/** `@tauri-apps/api/path` 的 web shim —— 浏览器无文件系统，返回占位。 */
export async function homeDir(): Promise<string> {
  return "";
}
export async function join(...parts: string[]): Promise<string> {
  return parts.join("/").replace(/\/+/g, "/");
}
export async function appConfigDir(): Promise<string> {
  return "";
}
export async function appDataDir(): Promise<string> {
  return "";
}
