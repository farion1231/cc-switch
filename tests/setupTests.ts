import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import { resetProviderState } from "./msw/state";
import "./msw/tauriMocks";

// A request without a handler fails the test that made it, instead of only
// logging while the component quietly takes its error path.
const unhandledRequests: string[] = [];

beforeAll(async () => {
  server.listen({
    onUnhandledRequest: (request) => {
      unhandledRequests.push(`${request.method} ${request.url}`);
    },
  });
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
  const unhandled = unhandledRequests.splice(0);
  if (unhandled.length > 0) {
    throw new Error(
      `Requests without an MSW handler (add one to tests/msw/handlers.ts or server.use it in the test):\n${unhandled.join("\n")}`,
    );
  }
});

afterAll(() => {
  server.close();
});
