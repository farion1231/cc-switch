import { useSyncExternalStore } from "react";
import type { TFunction } from "i18next";
import { settingsApi } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";

/**
 * 授权中心「登录时自动打开浏览器」：关掉后各服务的登录只复制登录链接，由用户自己选浏览器打开
 * （用独立浏览器环境管多个账号时，不让登录混进系统默认浏览器）。
 * 只是本机的界面偏好，记在 localStorage，不进 settings.json、不随云同步；读不到时按默认（打开）。
 */
const STORAGE_KEY = "cc-switch.authCenter.openBrowser";

const listeners = new Set<() => void>();

export function readAuthOpenBrowser(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) !== "0";
  } catch {
    return true;
  }
}

export function writeAuthOpenBrowser(open: boolean) {
  try {
    if (open) window.localStorage.removeItem(STORAGE_KEY);
    else window.localStorage.setItem(STORAGE_KEY, "0");
  } catch {
    // 写不了就只在本次生效
  }
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useAuthOpenBrowser() {
  return useSyncExternalStore(subscribe, readAuthOpenBrowser, () => true);
}

/**
 * 登录开始时把登录页交给用户：按偏好打开系统浏览器，或复制链接并提示。
 * 返回是否只复制了链接（设备码登录据此不再自动复制验证码，免得盖掉链接）。
 */
export async function deliverLoginLink(
  url: string,
  t: TFunction,
): Promise<{ linkOnly: boolean }> {
  if (readAuthOpenBrowser()) {
    try {
      await settingsApi.openExternal(url);
    } catch (e) {
      console.debug("[AuthCenter] Failed to open browser:", e);
    }
    return { linkOnly: false };
  }
  try {
    await copyText(url);
    toast.success(
      t("authCenter.pending.linkCopied", { defaultValue: "登录链接已复制。" }),
    );
  } catch (e) {
    console.debug("[AuthCenter] Failed to copy login link:", e);
  }
  return { linkOnly: true };
}
