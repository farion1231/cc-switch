import { readFileSync } from "node:fs";
import path from "node:path";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiKeyVarsSection } from "@/components/settings/sections/ApiKeyVarsSection";
import type { SettingsFormState } from "@/hooks/useSettings";
import { toast } from "@/lib/toast";
import { server } from "../msw/server";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { error?: string; path?: string }) =>
      `${key}${options?.error ? `: ${options.error}` : ""}${options?.path ? `: ${options.path}` : ""}`,
  }),
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const templateFiles = [
  "index.mjs",
  "config.mjs",
  "lib/provider.mjs",
  "lib/contract.mjs",
  "README.zh-CN.md",
  "README.md",
].map((file) => ({
  path: file,
  content: readFileSync(
    path.resolve("src-tauri/resources/api-key-var-template", file),
    "utf8",
  ),
}));

const downloadLabel = "apiKeyVars.template.download";
const guideLabel = "apiKeyVars.template.title";
let previewCalls = 0;

function renderSection() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const onAutoSave = vi.fn().mockResolvedValue(true);
  const result = render(
    <QueryClientProvider client={client}>
      <ApiKeyVarsSection
        settings={{} as SettingsFormState}
        onAutoSave={onAutoSave}
      />
    </QueryClientProvider>,
  );
  return { ...result, onAutoSave, user: userEvent.setup() };
}

beforeEach(() => {
  previewCalls = 0;
  server.use(
    http.post("http://tauri.local/list_api_key_vars", () =>
      HttpResponse.json([]),
    ),
    http.post("http://tauri.local/get_api_key_var_template", () => {
      previewCalls += 1;
      return HttpResponse.json(templateFiles);
    }),
  );
});

describe("API Key variable template UI and IPC", () => {
  it("loads the guide on demand and displays every complete template file", async () => {
    const { user } = renderSection();
    const headerButtons = screen.getAllByRole("button");
    expect(
      headerButtons.indexOf(
        screen.getByRole("button", { name: downloadLabel }),
      ),
    ).toBeLessThan(
      headerButtons.indexOf(
        screen.getByRole("button", { name: "apiKeyVars.add" }),
      ),
    );
    expect(screen.getByRole("button", { name: guideLabel })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(previewCalls).toBe(0);
    await user.click(screen.getByRole("button", { name: guideLabel }));
    expect(
      await screen.findByRole("tab", { name: "index.mjs" }),
    ).toBeInTheDocument();
    for (const file of templateFiles) {
      await user.click(screen.getByRole("tab", { name: file.path }));
      expect(
        screen.getByRole("tabpanel").querySelector("code")?.textContent,
      ).toBe(file.content);
    }
    expect(previewCalls).toBe(1);
    await user.click(screen.getByRole("button", { name: guideLabel }));
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  });

  it("downloads through the backend without arguments or configuration changes", async () => {
    let payload: unknown;
    server.use(
      http.post(
        "http://tauri.local/download_api_key_var_template",
        async ({ request }) => {
          payload = await request.json();
          return HttpResponse.json(
            "C:\\templates\\cc-switch-api-key-template.zip",
          );
        },
      ),
    );
    const { user, onAutoSave } = renderSection();
    await user.click(screen.getByRole("button", { name: downloadLabel }));
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        "apiKeyVars.template.downloaded: C:\\templates\\cc-switch-api-key-template.zip",
      ),
    );
    expect(payload).toEqual({});
    expect(onAutoSave).not.toHaveBeenCalled();
    expect(previewCalls).toBe(0);
  });

  it("silently handles cancellation and allows another download", async () => {
    let calls = 0;
    server.use(
      http.post("http://tauri.local/download_api_key_var_template", () => {
        calls += 1;
        return HttpResponse.json(null);
      }),
    );
    const { user } = renderSection();
    const button = screen.getByRole("button", { name: downloadLabel });
    await user.click(button);
    await waitFor(() => expect(button).toBeEnabled());
    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
    await user.click(button);
    await waitFor(() => expect(calls).toBe(2));
    await waitFor(() => expect(button).toBeEnabled());
  });

  it("blocks duplicate clicks and restores the button after a save failure", async () => {
    let calls = 0;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    server.use(
      http.post(
        "http://tauri.local/download_api_key_var_template",
        async () => {
          calls += 1;
          await pending;
          return new HttpResponse("disk is full", { status: 500 });
        },
      ),
    );
    renderSection();
    const button = screen.getByRole("button", { name: downloadLabel });
    fireEvent.click(button);
    fireEvent.click(button);
    await waitFor(() => expect(calls).toBe(1));
    expect(button).toBeDisabled();
    release();
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "apiKeyVars.template.downloadFailed: disk is full",
      ),
    );
    expect(button).toBeEnabled();
  });

  it("retries a failed preview and keeps downloads available", async () => {
    let calls = 0;
    server.use(
      http.post("http://tauri.local/get_api_key_var_template", () => {
        calls += 1;
        return calls === 1
          ? new HttpResponse("preview unavailable", { status: 500 })
          : HttpResponse.json(templateFiles);
      }),
    );
    const { user } = renderSection();
    await user.click(screen.getByRole("button", { name: guideLabel }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "preview unavailable",
    );
    expect(screen.getByRole("button", { name: downloadLabel })).toBeEnabled();
    await user.click(
      screen.getByRole("button", { name: "apiKeyVars.template.retry" }),
    );
    expect(
      await screen.findByRole("tab", { name: "index.mjs" }),
    ).toBeInTheDocument();
    expect(calls).toBe(2);
  });

  it("opens the same guide above the variable dialog and preserves the form", async () => {
    const { user } = renderSection();
    await user.click(screen.getByRole("button", { name: "apiKeyVars.add" }));
    const nameInput = screen.getByPlaceholderText("MY_KEY");
    await user.type(nameInput, "EXAMPLE_KEY");
    await user.click(
      screen.getByRole("button", { name: "apiKeyVars.template.openGuide" }),
    );
    const guideDialog = screen.getByRole("dialog", { name: guideLabel });
    expect(
      await within(guideDialog).findByRole("tab", { name: "index.mjs" }),
    ).toBeInTheDocument();
    expect(
      within(guideDialog).getByRole("button", { name: downloadLabel }),
    ).toBeEnabled();
    await user.click(
      within(guideDialog).getByRole("button", { name: "common.close" }),
    );
    expect(
      screen.getByRole("dialog", { name: "apiKeyVars.addTitle" }),
    ).toBeInTheDocument();
    expect(screen.getByPlaceholderText("MY_KEY")).toHaveValue("EXAMPLE_KEY");
    expect(previewCalls).toBe(1);
  });
});
