import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { SkillStorageLocationSettings } from "@/components/settings/SkillStorageLocationSettings";

const TAURI_ENDPOINT = "http://tauri.local";

// 测试环境的 i18n 资源为空：用回显 mock 锁定「真实路径被送进 t()」
const tMock = vi.fn(
  (key: string, opts?: { path?: string }) =>
    opts?.path ? `${key}:${opts.path}` : key,
);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

beforeEach(() => {
  tMock.mockClear();
});

describe("SkillStorageLocationSettings", () => {
  it("shows the resolved storage dir (config-dir aware) in the cc_switch hint", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_skill_storage_dir`, async ({ request }) => {
        const payload = (await request.json()) as { location?: string };
        expect(payload.location).toBe("cc_switch");
        return HttpResponse.json("D:\\Soft_Config\\.cc-switch\\skills");
      }),
    );
    renderWithProviders(
      <SkillStorageLocationSettings
        value="cc_switch"
        installedCount={0}
        onMigrated={vi.fn()}
      />,
    );
    // 回显 mock 把插值路径拼在 key 后：路径来自后端解析而非写死的 ~/.cc-switch
    await waitFor(() =>
      expect(
        screen.getByText(
          "settings.skillStorage.ccSwitchHint:D:\\Soft_Config\\.cc-switch\\skills",
        ),
      ).toBeInTheDocument(),
    );
  });

  it("keeps the unified branch on its own hint without a path lookup", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_skill_storage_dir`, () =>
        HttpResponse.json("/home/user/.agents/skills"),
      ),
    );
    renderWithProviders(
      <SkillStorageLocationSettings
        value="unified"
        installedCount={0}
        onMigrated={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(
        screen.getByText("settings.skillStorage.unifiedHint"),
      ).toBeInTheDocument(),
    );
    expect(
      tMock.mock.calls.some(([key]) => key === "settings.skillStorage.ccSwitchHint"),
    ).toBe(false);
  });
});
