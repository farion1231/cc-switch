/** `@tauri-apps/api/app` 的 web shim。 */
export async function getVersion(): Promise<string> {
  return import.meta.env.VITE_APP_VERSION ?? "web";
}
export async function getName(): Promise<string> {
  return "cc-switch-web";
}
export async function getTauriVersion(): Promise<string> {
  return "0.0.0-web";
}
