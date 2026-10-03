/**
 * 新增对话框：Codex、Gemini CLI、Grok Build 的表单把预设或模板投影到当前配置文件上显示，
 * 投影结果就是保存时三方比较的底。锁定一条不变式：每次重置显示内容之后，编辑框里的内容
 * 和最后上报的底一致。对不上时，底里有、显示里没有的全局设置会在保存时被当成删除写进
 * 配置文件。
 *
 * MSW 替身：投影在原内容上加一份「配置文件里已有的全局设置」，模拟真实的 live。
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import { grokBuildProviderPresets } from "@/config/grokBuildProviderPresets";
import type { AppId } from "@/lib/api";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: null,
      accounts: [],
    }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
  };
});

const LIVE_TOML = '\n[ui]\ntheme = "dark"\n';

type Base = Record<string, unknown> | null;

function projectAsLive() {
  server.use(
    http.post(
      "http://tauri.local/get_provider_editor_view",
      async ({ request }) => {
        const { app, settingsConfig = {} } = (await request.json()) as {
          app: AppId;
          settingsConfig?: Record<string, unknown>;
        };
        const settings = { ...settingsConfig };
        if (app === "gemini") {
          settings.env = {
            ...(settingsConfig.env as Record<string, unknown>),
            GEMINI_SANDBOX: "docker",
          };
          settings.config = {
            ...(settingsConfig.config as Record<string, unknown>),
            ui: { theme: "dark" },
          };
        } else {
          settings.config = `${String(settingsConfig.config ?? "")}${LIVE_TOML}`;
        }
        return HttpResponse.json({ settings, inactive: [] });
      },
    ),
  );
}

function renderForm(appId: AppId, bases: Base[]) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId={appId}
        submitLabel="save"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        onEditorBaseChange={(base) => bases.push(base)}
      />
    </QueryClientProvider>,
  );
}

function clickPreset(name: string) {
  const matches = screen
    .getAllByRole("button")
    .filter(
      (button) => button.querySelector("span.truncate")?.textContent === name,
    );
  expect(matches, `预设按钮「${name}」应唯一`).toHaveLength(1);
  fireEvent.click(matches[0]);
}

function editorTexts(): string[] {
  return screen
    .getAllByTestId("json-editor")
    .map((node) => (node as HTMLTextAreaElement).value);
}

/** 等到最后一次上报的底不为空，并且和上一次检查时不同。 */
async function nextBase(bases: Base[], seen: number): Promise<Base> {
  await waitFor(() => {
    expect(bases.length).toBeGreaterThan(seen);
    expect(bases.at(-1)).not.toBeNull();
  });
  return bases.at(-1)!;
}

describe("新增对话框的草稿投影", () => {
  beforeEach(() => {
    projectAsLive();
  });

  it("Codex：打开、选预设、切回自定义，显示内容始终等于底", async () => {
    const bases: Base[] = [];
    renderForm("codex", bases);

    let base = await nextBase(bases, 0);
    await waitFor(() =>
      expect(editorTexts()).toContain(String(base!.config)),
    );

    const preset = codexProviderPresets.find(
      (item) => item.category !== "official" && item.name === "Nvidia",
    )!;
    let seen = bases.length;
    clickPreset(preset.name);
    base = await nextBase(bases, seen);
    expect(String(base!.config)).toContain(LIVE_TOML.trim());
    await waitFor(() =>
      expect(editorTexts()).toContain(String(base!.config)),
    );

    seen = bases.length;
    clickPreset("providerPreset.custom");
    base = await nextBase(bases, seen);
    expect(String(base!.config)).not.toContain("nvidia");
    await waitFor(() =>
      expect(editorTexts()).toContain(String(base!.config)),
    );
  });

  it("Gemini：选预设、切回自定义后，全局设置仍在显示内容和底里", async () => {
    const bases: Base[] = [];
    renderForm("gemini", bases);
    await nextBase(bases, 0);

    const preset = geminiProviderPresets.find(
      (item) => item.category !== "official",
    )!;
    let seen = bases.length;
    clickPreset(preset.name);
    let base = await nextBase(bases, seen);
    expect((base!.env as Record<string, unknown>).GEMINI_SANDBOX).toBe(
      "docker",
    );

    seen = bases.length;
    clickPreset("providerPreset.custom");
    base = await nextBase(bases, seen);
    expect((base!.env as Record<string, unknown>).GEMINI_SANDBOX).toBe(
      "docker",
    );
    await waitFor(() =>
      expect(
        editorTexts().some((text) => text.includes("GEMINI_SANDBOX=docker")),
      ).toBe(true),
    );
  });

  it("Grok Build：选预设后显示投影内容，切到官方卡作废底", async () => {
    const bases: Base[] = [];
    renderForm("grokbuild", bases);
    await nextBase(bases, 0);

    const preset = grokBuildProviderPresets[0];
    const seen = bases.length;
    clickPreset(preset.nameKey ?? preset.name);
    const base = await nextBase(bases, seen);
    await waitFor(() =>
      expect(editorTexts()).toContain(String(base!.config)),
    );

    fireEvent.click(screen.getByRole("button", { name: /Grok Official/ }));
    await waitFor(() => expect(bases.at(-1)).toBeNull());
  });
});
