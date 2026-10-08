import { extractErrorMessage } from "@/utils/errorUtils";

/** 后端 `LiveWriteError::EditConflict` 的错误码。 */
export const LIVE_EDIT_CONFLICT_CODE = "LIVE_EDIT_CONFLICT";

export interface LiveEditConflict {
  /** 编辑期间被别的程序改过的键路径，比如 `hooks`、`env.DEBUG`。 */
  keys: string[];
}

/**
 * 编辑器保存时，这些键在窗口打开之后被别的程序（比如 Claude Code）改过。
 * 后端返回 JSON 字符串，前端据此让用户选保留哪一边。
 */
export function parseLiveEditConflict(error: unknown): LiveEditConflict | null {
  const message = extractErrorMessage(error);
  if (!message.includes(LIVE_EDIT_CONFLICT_CODE)) return null;
  try {
    const parsed = JSON.parse(message) as { code?: unknown; keys?: unknown };
    if (
      parsed.code !== LIVE_EDIT_CONFLICT_CODE ||
      !Array.isArray(parsed.keys)
    ) {
      return null;
    }
    return { keys: parsed.keys.map(String) };
  } catch {
    return null;
  }
}
