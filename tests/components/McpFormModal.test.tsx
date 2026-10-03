import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { McpServer } from "@/types";
import McpFormModal from "@/components/mcp/McpFormModal";
import { MCP_APP_IDS } from "@/config/appConfig";

const mocks = vi.hoisted(() => ({
  upsert: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: { error: mocks.toastError, success: mocks.toastSuccess },
}));

vi.mock("@/config/mcpPresets", () => ({
  mcpPresets: [
    {
      id: "preset-stdio",
      name: "Preset Server",
      server: { type: "stdio", command: "preset-cmd", args: ["--x"] },
      homepage: "https://preset.dev",
    },
  ],
  getMcpPresetWithDescription: (preset: any) => ({
    ...preset,
    description: "Preset description",
    tags: ["preset"],
  }),
}));

vi.mock("@/hooks/useMcp", () => ({
  useUpsertMcpServer: () => ({ mutateAsync: mocks.upsert }),
}));

const ALL_ON = Object.fromEntries(MCP_APP_IDS.map((app) => [app, true]));

function renderForm(
  props: Partial<React.ComponentProps<typeof McpFormModal>> = {},
) {
  const onSave = vi.fn();
  const onClose = vi.fn();
  render(
    <McpFormModal
      onSave={onSave}
      onClose={onClose}
      existingServers={{}}
      {...props}
    />,
  );
  return { onSave, onClose };
}

const el = (id: string) => document.getElementById(id) as HTMLInputElement;

const addButton = () => screen.getByRole("button", { name: "common.add" });

describe("McpFormModal (drawer)", () => {
  beforeEach(() => {
    mocks.upsert.mockReset();
    mocks.upsert.mockResolvedValue(undefined);
  });

  it("fills the form from a template", async () => {
    renderForm();
    await userEvent.click(screen.getByRole("button", { name: "preset-stdio" }));
    expect(el("mcp-name")).toHaveValue("preset-stdio");
    expect(el("mcp-cmd")).toHaveValue("preset-cmd");
    expect(screen.getByLabelText("mcpPage.drawer.argAria")).toHaveValue("--x");
  });

  it("adds a stdio server with every visible app ticked by default", async () => {
    const { onSave } = renderForm();
    fireEvent.change(el("mcp-name"), {
      target: { value: "my-server" },
    });
    fireEvent.change(el("mcp-cmd"), {
      target: { value: "uvx" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.drawer.addArg" }),
    );
    fireEvent.change(screen.getByLabelText("mcpPage.drawer.argAria"), {
      target: { value: "mcp-server-fetch" },
    });
    await userEvent.click(addButton());

    await waitFor(() => expect(mocks.upsert).toHaveBeenCalledTimes(1));
    const entry = mocks.upsert.mock.calls[0][0] as McpServer;
    expect(entry).toMatchObject({
      id: "my-server",
      name: "my-server",
      server: { type: "stdio", command: "uvx", args: ["mcp-server-fetch"] },
    });
    expect(entry.apps).toMatchObject({ ...ALL_ON, openclaw: false });
    expect(onSave).toHaveBeenCalled();
  });

  it("shows field errors instead of saving when required fields are empty", async () => {
    renderForm();
    await userEvent.click(addButton());
    expect(
      screen.getByText("mcpPage.drawer.errors.nameRequired"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("mcpPage.drawer.errors.commandRequired"),
    ).toBeInTheDocument();
    expect(mocks.upsert).not.toHaveBeenCalled();
    await waitFor(() => expect(document.activeElement).toBe(el("mcp-name")));
  });

  it("flags a duplicate name as soon as it is typed", () => {
    renderForm({
      existingServers: {
        fetch: {
          id: "fetch",
          name: "fetch",
          server: { command: "uvx" },
          apps: {} as McpServer["apps"],
        },
      },
    });
    fireEvent.change(el("mcp-name"), {
      target: { value: "fetch" },
    });
    expect(
      screen.getByText("mcpPage.drawer.errors.nameExists"),
    ).toBeInTheDocument();
  });

  it("recognizes a pasted mcpServers block and clears the paste box", () => {
    renderForm();
    const paste = screen.getByLabelText("mcpPage.drawer.paste");
    fireEvent.change(paste, {
      target: {
        value: JSON.stringify({
          mcpServers: {
            linear: {
              type: "http",
              url: "https://mcp.linear.app/mcp",
              headers: { Authorization: "Bearer secret" },
            },
          },
        }),
      },
    });
    expect(paste).toHaveValue("");
    expect(el("mcp-name")).toHaveValue("linear");
    expect(el("mcp-url")).toHaveValue("https://mcp.linear.app/mcp");
    // 请求头的值是密码框
    expect(screen.getByLabelText("mcpPage.drawer.kvValueAria")).toHaveAttribute(
      "type",
      "password",
    );
  });

  it("adds only new servers from a batch paste and skips existing names", async () => {
    renderForm({
      existingServers: {
        figma: {
          id: "figma",
          name: "figma",
          server: { type: "http", url: "http://127.0.0.1:3845/mcp" },
          apps: { claude: true } as McpServer["apps"],
        },
      },
    });
    fireEvent.change(screen.getByLabelText("mcpPage.drawer.paste"), {
      target: {
        value: JSON.stringify({
          mcpServers: {
            linear: { type: "http", url: "https://mcp.linear.app/mcp" },
            figma: { type: "http", url: "http://127.0.0.1:3845/mcp" },
          },
        }),
      },
    });
    expect(screen.getByText("mcpPage.drawer.batchTitle")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.drawer.addMany" }),
    );
    await waitFor(() => expect(mocks.upsert).toHaveBeenCalledTimes(1));
    expect(mocks.upsert.mock.calls[0][0]).toMatchObject({ id: "linear" });
  });

  it("keeps hidden apps' flags when overwriting an existing server from a batch", async () => {
    // Hermes 从侧栏隐藏了：覆盖 figma 时它在 Hermes 的开关要原样带回，不能被当成关掉
    renderForm({
      visibleAppIds: ["claude", "codex"],
      existingServers: {
        figma: {
          id: "figma",
          name: "figma",
          server: { type: "http", url: "http://127.0.0.1:3845/mcp" },
          apps: { claude: false, hermes: true } as McpServer["apps"],
        },
      },
    });
    fireEvent.change(screen.getByLabelText("mcpPage.drawer.paste"), {
      target: {
        value: JSON.stringify({
          mcpServers: {
            linear: { type: "http", url: "https://mcp.linear.app/mcp" },
            figma: { type: "http", url: "http://127.0.0.1:3845/v2" },
          },
        }),
      },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.drawer.batchOverwrite" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.drawer.addManyReplace" }),
    );
    await waitFor(() => expect(mocks.upsert).toHaveBeenCalledTimes(2));
    const figma = mocks.upsert.mock.calls
      .map((call) => call[0])
      .find((entry) => entry.id === "figma");
    expect(figma.apps).toMatchObject({
      claude: true,
      codex: true,
      hermes: true,
    });
  });

  it("masks secrets in the JSON tab and keeps them on save", async () => {
    const server: McpServer = {
      id: "serena",
      name: "serena",
      server: {
        type: "stdio",
        command: "uvx",
        env: { TOKEN: "keep-me" },
        startup_timeout_ms: 30000,
      },
      apps: { claude: true, codex: false, hermes: true } as McpServer["apps"],
    };
    renderForm({ editingId: "serena", initialData: server });

    expect(el("mcp-name")).toHaveAttribute("readonly");
    await userEvent.click(screen.getByRole("button", { name: "JSON" }));
    const json = screen.getByLabelText("mcpPage.drawer.jsonLabel");
    expect((json as HTMLTextAreaElement).value).toContain("••••••••");
    expect((json as HTMLTextAreaElement).value).not.toContain("keep-me");

    fireEvent.change(json, {
      target: {
        value: (json as HTMLTextAreaElement).value.replace('"uvx"', '"npx"'),
      },
    });
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.upsert).toHaveBeenCalledTimes(1));
    expect(mocks.upsert.mock.calls[0][0]).toMatchObject({
      id: "serena",
      server: {
        type: "stdio",
        command: "npx",
        env: { TOKEN: "keep-me" },
        startup_timeout_ms: 30000,
      },
      apps: { claude: true, codex: false, hermes: true },
    });
  });

  it("keeps the form tab disabled while the JSON has a syntax error", async () => {
    renderForm({
      editingId: "x",
      initialData: {
        id: "x",
        name: "x",
        server: { type: "stdio", command: "uvx" },
        apps: {} as McpServer["apps"],
      },
    });
    await userEvent.click(screen.getByRole("button", { name: "JSON" }));
    fireEvent.change(screen.getByLabelText("mcpPage.drawer.jsonLabel"), {
      target: { value: "{ broken" },
    });
    expect(
      screen.getByRole("button", { name: "mcpPage.drawer.tabForm" }),
    ).toBeDisabled();
  });

  it("only lists visible apps but preserves hidden ones when editing", async () => {
    renderForm({
      editingId: "x",
      visibleAppIds: ["claude", "codex"],
      initialData: {
        id: "x",
        name: "x",
        server: { type: "stdio", command: "uvx" },
        apps: { claude: false, hermes: true } as McpServer["apps"],
      },
    });
    expect(screen.queryByText("Hermes")).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("checkbox", { name: /Claude Code/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.upsert).toHaveBeenCalled());
    expect(mocks.upsert.mock.calls[0][0].apps).toMatchObject({
      claude: true,
      hermes: true,
    });
  });

  it("reports a save failure and keeps the drawer open", async () => {
    mocks.upsert.mockRejectedValueOnce(new Error("disk full"));
    const { onSave } = renderForm();
    fireEvent.change(el("mcp-name"), {
      target: { value: "a" },
    });
    fireEvent.change(el("mcp-cmd"), {
      target: { value: "uvx" },
    });
    await userEvent.click(addButton());
    await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
    expect(onSave).not.toHaveBeenCalled();
    expect(addButton()).toBeEnabled();
  });
});
