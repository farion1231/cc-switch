import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup, configure } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import { resetProviderState } from "./msw/state";
import "./msw/tauriMocks";

// `waitFor` 的默认预算只有 1000ms，那假设的是一台空机器。全量 140 个文件并行、
// 机器满载时，一次「渲染 + MSW 往返」就能超过它：实测
// `SettingsPage integration > loads default settings from MSW` 在
// `language:zh` 上等了 1404ms 就判失败，而它单独跑永远是毫秒级。5s 与
// vitest.config.ts 里的 testTimeout 是同一个理由的两处旋钮。
configure({ asyncUtilTimeout: 5000 });

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "warn" });
  await i18n.use(initReactI18next).init({
    lng: "zh",
    fallbackLng: "zh",
    resources: {
      zh: { translation: {} },
      en: { translation: {} },
    },
    interpolation: {
      escapeValue: false,
    },
  });
});

afterEach(() => {
  cleanup();
  // 页面数据的本地缓存（src/lib/localCache.ts）和用量页记住的选择不能从一个用例带到下一个
  for (const key of Object.keys(localStorage)) {
    if (
      key.startsWith("cc-switch-cache:") ||
      key.startsWith("cc-switch.usage.")
    )
      localStorage.removeItem(key);
  }
  resetProviderState();
  server.resetHandlers();
  vi.clearAllMocks();
});

afterAll(() => {
  server.close();
});
