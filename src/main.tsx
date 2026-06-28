import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { UpdateProvider } from "./contexts/UpdateContext";
import "./index.css";
import i18n from "./i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { isTauri } from "@/lib/environment";
import { installWebLogger, webLog } from "@/lib/webLogger";

// Install browser-side error/console capture as early as possible so failures
// during bootstrap are reported to the backend (web mode only; no-op in Tauri).
installWebLogger();

try {
  const ua = navigator.userAgent || "";
  const plat = (navigator.platform || "").toLowerCase();
  const isMac = /mac/i.test(ua) || plat.includes("mac");
  if (isMac) {
    document.body.classList.add("is-mac");
  }
} catch {}

interface ConfigLoadErrorPayload {
  path?: string;
  error?: string;
}

async function handleConfigLoadError(
  payload: ConfigLoadErrorPayload | null,
): Promise<void> {
  const path = payload?.path ?? "~/.cc-switch/config.json";
  const detail = payload?.error ?? "Unknown error";

  if (!isTauri()) {
    alert(
      i18n.t("errors.configLoadFailedMessage", {
        path,
        detail,
        defaultValue:
          "无法读取配置文件：\n{{path}}\n\n错误详情：\n{{detail}}\n\n请手动检查 JSON 是否有效，或从同目录的备份文件（如 config.json.bak）恢复。",
      }),
    );
    return;
  }

  const { message } = await import("@tauri-apps/plugin-dialog");
  const { exit } = await import("@tauri-apps/plugin-process");

  await message(
    i18n.t("errors.configLoadFailedMessage", {
      path,
      detail,
      defaultValue:
        "无法读取配置文件：\n{{path}}\n\n错误详情：\n{{detail}}\n\n请手动检查 JSON 是否有效，或从同目录的备份文件（如 config.json.bak）恢复。\n\n应用将退出以便您进行修复。",
    }),
    {
      title: i18n.t("errors.configLoadFailedTitle", {
        defaultValue: "配置加载失败",
      }),
      kind: "error",
    },
  );

  await exit(1);
}

if (isTauri()) {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    void listen("configLoadError", async (evt) => {
      await handleConfigLoadError(evt.payload as ConfigLoadErrorPayload | null);
    });
  } catch (e) {
    console.error("订阅 configLoadError 事件失败", e);
  }
}

async function bootstrap() {
  if (isTauri()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const initError = (await invoke(
        "get_init_error",
      )) as ConfigLoadErrorPayload | null;
      if (initError && (initError.path || initError.error)) {
        await handleConfigLoadError(initError);
        return;
      }
    } catch (e) {
      console.error("拉取初始化错误失败", e);
    }
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <UpdateProvider>
            <App />
            <Toaster />
          </UpdateProvider>
        </ThemeProvider>
      </QueryClientProvider>
    </React.StrictMode>,
  );

  webLog.info("app mounted", { mode: isTauri() ? "tauri" : "web" });
}

void bootstrap();
