/** `@tauri-apps/plugin-dialog` 的 web shim —— 用浏览器原生对话框。 */
export interface MessageOptions {
  title?: string;
  message: string;
  kind?: "info" | "warning" | "error";
}
export async function message(opts: string | MessageOptions): Promise<void> {
  const text = typeof opts === "string" ? opts : opts.message;
  window.alert(text);
}
export async function ask(opts: string | MessageOptions): Promise<boolean> {
  const text = typeof opts === "string" ? opts : opts.message;
  return window.confirm(text);
}
export async function confirm(opts: string | MessageOptions): Promise<boolean> {
  const text = typeof opts === "string" ? opts : opts.message;
  return window.confirm(text);
}
export async function open(): Promise<string | string[] | null> {
  return null;
}
export async function save(): Promise<string | null> {
  return null;
}
