/** `@tauri-apps/plugin-process` 的 web shim —— 浏览器无法退出进程。 */
export async function exit(_code = 0): Promise<void> {
  try {
    window.close();
  } catch {
    // window.close() 在非 script-open 的窗口会抛；静默。
  }
}
export async function relaunch(): Promise<void> {
  location.reload();
}
