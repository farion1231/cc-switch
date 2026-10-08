import { toast as sonnerToast, type ExternalToast } from "sonner";

/**
 * sonner 的 toast，加一层去重：同一句话还在屏幕上时再弹一次，就原地刷新那条（计时重来），
 * 不再往上叠一摞一样的（反复点「检查更新」之类的按钮）。
 * 其余用法和 sonner 完全一样，业务代码统一从这里 import。
 */

type Kind = "success" | "error" | "info" | "warning";
type Message = Parameters<typeof sonnerToast.success>[0];

/** 和 <Toaster duration> 一致 */
const DEFAULT_DURATION = 2000;
const active = new Map<string, { id: string | number; until: number }>();

function show(kind: Kind, message: Message, options?: ExternalToast) {
  const key = typeof message === "string" ? `${kind}:${message}` : undefined;
  const now = Date.now();
  const previous = key ? active.get(key) : undefined;
  const reuse =
    previous &&
    previous.id !== undefined &&
    previous.until > now &&
    options?.id === undefined
      ? { ...options, id: previous.id }
      : options;
  const id = reuse
    ? sonnerToast[kind](message, reuse)
    : sonnerToast[kind](message);
  if (key) {
    const duration = options?.duration ?? DEFAULT_DURATION;
    active.set(key, {
      id: reuse?.id ?? id,
      until: Number.isFinite(duration) ? now + duration : Infinity,
    });
  }
  return id;
}

export const toast = Object.assign(
  (message: Message, options?: ExternalToast) => sonnerToast(message, options),
  sonnerToast,
  {
    success: (message: Message, options?: ExternalToast) =>
      show("success", message, options),
    error: (message: Message, options?: ExternalToast) =>
      show("error", message, options),
    info: (message: Message, options?: ExternalToast) =>
      show("info", message, options),
    warning: (message: Message, options?: ExternalToast) =>
      show("warning", message, options),
  },
);
