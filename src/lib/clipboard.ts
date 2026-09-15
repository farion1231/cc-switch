import { invoke } from "@tauri-apps/api/core";

export async function copyText(text: string): Promise<void> {
  try {
    await invoke("copy_text_to_clipboard", { text });
    return;
  } catch (nativeError) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch (webError) {
      throw webError instanceof Error
        ? webError
        : nativeError instanceof Error
          ? nativeError
          : new Error(String(webError || nativeError));
    }
  }
}

/**
 * 从系统剪贴板读取文本。优先调用原生命令（与 copyText 对称），
 * 失败时回退到 Web Clipboard API。
 */
export async function readText(): Promise<string> {
  try {
    return await invoke("read_text_from_clipboard");
  } catch (nativeError) {
    try {
      return await navigator.clipboard.readText();
    } catch (webError) {
      throw webError instanceof Error
        ? webError
        : nativeError instanceof Error
          ? nativeError
          : new Error(String(webError || nativeError));
    }
  }
}
