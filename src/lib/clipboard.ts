import { invoke } from "@tauri-apps/api/core";
import { isLinux } from "@/lib/platform";

async function copyViaNative(text: string): Promise<void> {
  await invoke("copy_text_to_clipboard", { text });
}

async function copyViaWeb(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

// Linux 上先走 WebView 的剪贴板：arboard 写完即释放剪贴板所有权，
// 没有剪贴板管理器接手时（如 KDE Wayland）会“提示成功、剪贴板为空”，
// 而且后端不报错，前端也就不会回落。GTK 会一直持有 WebView 写入的内容（#9384）。
// macOS / Windows 仍先走后端，WebView 剪贴板在失去焦点等情况下会被拒绝。
export async function copyText(text: string): Promise<void> {
  const [first, second] = isLinux()
    ? [copyViaWeb, copyViaNative]
    : [copyViaNative, copyViaWeb];
  try {
    await first(text);
    return;
  } catch (firstError) {
    try {
      await second(text);
      return;
    } catch (secondError) {
      throw secondError instanceof Error
        ? secondError
        : firstError instanceof Error
          ? firstError
          : new Error(String(secondError || firstError));
    }
  }
}
