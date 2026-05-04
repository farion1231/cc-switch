import { invokeLocal } from "@/lib/api/transport";
import { isCliWebUi } from "@/lib/platform";

export async function copyText(text: string): Promise<void> {
  if (isCliWebUi()) {
    await navigator.clipboard.writeText(text);
    return;
  }

  try {
    await invokeLocal("copy_text_to_clipboard", { text });
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
