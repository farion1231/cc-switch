/** `@tauri-apps/plugin-log` 的 web shim —— 转发到 console。 */
export async function trace(msg: unknown, _opts?: unknown): Promise<void> {
  console.trace(msg);
}
export async function debug(msg: unknown, _opts?: unknown): Promise<void> {
  console.debug(msg);
}
export async function info(msg: unknown, _opts?: unknown): Promise<void> {
  console.info(msg);
}
export async function warn(msg: unknown, _opts?: unknown): Promise<void> {
  console.warn(msg);
}
export async function error(msg: unknown, _opts?: unknown): Promise<void> {
  console.error(msg);
}
export async function attachLogger(): Promise<void> {}
