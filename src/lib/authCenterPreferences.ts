import { useSyncExternalStore } from "react";
import type { AuthCenterTarget } from "@/lib/api/auth";

export const AUTH_CENTER_APPS = [
  { id: "claude_official", name: "Claude" },
  { id: "github_copilot", name: "GitHub Copilot" },
  { id: "codex_oauth", name: "ChatGPT" },
  { id: "xai_oauth", name: "xAI" },
  { id: "kimi_oauth", name: "Kimi Code" },
  { id: "dsh_account", name: "DeepSeek Harness" },
  { id: "codebuddy_oauth", name: "CodeBuddy" },
  { id: "agy_oauth", name: "Antigravity" },
  { id: "workbuddy_account", name: "WorkBuddy" },
] as const satisfies ReadonlyArray<{ id: AuthCenterTarget; name: string }>;

export interface AuthCenterPreferences {
  order: AuthCenterTarget[];
  customOrder: boolean;
  hidden: AuthCenterTarget[];
  shown: AuthCenterTarget[];
}

export const DEFAULT_AUTH_CENTER_PREFERENCES: AuthCenterPreferences = {
  order: AUTH_CENTER_APPS.map(({ id }) => id),
  customOrder: false,
  hidden: [],
  shown: [],
};

// 和自动打开浏览器一样，这是本机的界面偏好，不影响账号、供应商或云同步。
const STORAGE_KEY = "cc-switch.authCenter.apps";
const listeners = new Set<() => void>();
let cachedRaw: string | null = null;
let cachedPreferences = DEFAULT_AUTH_CENTER_PREFERENCES;
let storageUnavailable = false;

function normalize(value: unknown): AuthCenterPreferences {
  const saved = value as Partial<AuthCenterPreferences> | null;
  const knownIds = DEFAULT_AUTH_CENTER_PREFERENCES.order;
  const validIds = (ids: unknown): AuthCenterTarget[] =>
    Array.isArray(ids) ? ids.filter((id) => knownIds.includes(id)) : [];
  const hidden = [...new Set(validIds(saved?.hidden))];
  return {
    // 去重并补上新版本新增的应用，损坏或过时的偏好不会让应用丢失。
    order: [...new Set([...validIds(saved?.order), ...knownIds])],
    customOrder:
      typeof saved?.customOrder === "boolean"
        ? saved.customOrder
        : validIds(saved?.order).length > 0,
    hidden,
    shown: [...new Set(validIds(saved?.shown))].filter(
      (id) => !hidden.includes(id),
    ),
  };
}

export function readAuthCenterPreferences(): AuthCenterPreferences {
  if (storageUnavailable) return cachedPreferences;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw !== cachedRaw) {
      cachedRaw = raw;
      try {
        cachedPreferences = normalize(raw ? JSON.parse(raw) : null);
      } catch {
        cachedPreferences = DEFAULT_AUTH_CENTER_PREFERENCES;
      }
    }
  } catch {
    // 无法访问存储时仍保留本次会话中的设置。
  }
  return cachedPreferences;
}

export function updateAuthCenterPreferences(
  update: (current: AuthCenterPreferences) => AuthCenterPreferences,
) {
  cachedPreferences = normalize(update(readAuthCenterPreferences()));
  cachedRaw = JSON.stringify(cachedPreferences);
  try {
    window.localStorage.setItem(STORAGE_KEY, cachedRaw);
  } catch {
    storageUnavailable = true;
  }
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  const onStorage = (event: StorageEvent) => {
    if (event.key === STORAGE_KEY || event.key === null) listener();
  };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", onStorage);
  };
}

export function useAuthCenterPreferences() {
  return useSyncExternalStore(
    subscribe,
    readAuthCenterPreferences,
    () => DEFAULT_AUTH_CENTER_PREFERENCES,
  );
}
