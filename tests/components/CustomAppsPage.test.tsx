import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { CustomAppsPage } from "@/components/custom-apps/CustomAppsPage";

const listener = vi.hoisted(() => ({ address: "127.0.0.1" }));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    status: { running: true, address: listener.address, port: 15721 },
  }),
}));

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

beforeEach(() => {
  invoke.mockReset();
  listener.address = "127.0.0.1";
});

it("creates, renames and deletes a profile and edits, switches and deletes its provider", async () => {
  const user = userEvent.setup();
  const app = { id: "meetings", name: "Meeting summaries" };
  let created = false;
  let added = false;
  let selected = false;
  let model = "qwen";
  invoke.mockImplementation(async (command, args) => {
    if (command === undefined) return;
    switch (command) {
      case "list_custom_apps":
        return created ? [app] : [];
      case "create_custom_app":
        created = true;
        return { ...app, token: "fixture-client-token" };
      case "list_custom_providers":
        return {
          providers: added
            ? [
                {
                  id: "local",
                  name: "Local",
                  model,
                  baseUrl: "http://127.0.0.1:11434/v1",
                },
              ]
            : [],
          selected: selected ? "local" : null,
        };
      case "rename_custom_app":
        app.name = args.name;
        return;
      case "update_custom_provider":
        model = args.provider.model;
        return;
      case "delete_custom_provider":
        added = false;
        selected = false;
        return;
      case "delete_custom_app":
        created = false;
        return;
      case "add_custom_provider":
        added = true;
        return;
      case "select_custom_provider":
        selected = true;
        return;
      default:
        throw new Error(`Unexpected Tauri command: ${command}`);
    }
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  function Page() {
    const [id, setId] = useState<string | null>(null);
    return <CustomAppsPage appId={id} onSelectApp={setId} />;
  }
  render(
    <QueryClientProvider client={queryClient}>
      <Page />
    </QueryClientProvider>,
  );
  await user.type(
    await screen.findByRole("textbox", { name: "customApps.appName" }),
    app.name,
  );
  await user.click(screen.getByRole("button", { name: "customApps.addApp" }));
  expect(
    await screen.findByDisplayValue("fixture-client-token"),
  ).toBeInTheDocument();
  expect(
    screen.getByDisplayValue("http://127.0.0.1:15721/apps/meetings/v1"),
  ).toBeInTheDocument();

  await user.type(
    screen.getByRole("textbox", { name: "customApps.providerName" }),
    "Local",
  );
  await user.type(
    screen.getByRole("textbox", { name: "customApps.baseUrl" }),
    "http://127.0.0.1:11434/v1",
  );
  await user.type(
    screen.getByRole("textbox", { name: "customApps.model" }),
    "qwen",
  );
  await user.click(
    screen.getByRole("button", { name: "customApps.addProvider" }),
  );
  await user.click(
    await screen.findByRole("button", { name: "customApps.switch" }),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "customApps.inUse" }),
    ).toBeDisabled(),
  );
  expect(invoke).toHaveBeenCalledWith("select_custom_provider", {
    appId: app.id,
    providerId: "local",
  });
  expect(invoke).toHaveBeenCalledWith("create_custom_app", { name: app.name });
  const name = screen.getByRole("textbox", { name: "customApps.appName" });
  await user.clear(name);
  await user.type(name, "Updated notes");
  await user.click(
    screen.getByRole("button", { name: "customApps.renameApp" }),
  );
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("rename_custom_app", {
      appId: app.id,
      name: "Updated notes",
    }),
  );
  await user.click(
    screen.getByRole("button", { name: "customApps.editProvider" }),
  );
  const modelInput = screen.getByRole("textbox", { name: "customApps.model" });
  expect(modelInput).toHaveValue("qwen");
  await user.clear(modelInput);
  await user.type(modelInput, "new-model");
  await user.click(
    screen.getByRole("button", { name: "customApps.saveProvider" }),
  );
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("update_custom_provider", {
      appId: app.id,
      providerId: "local",
      provider: {
        name: "Local",
        baseUrl: "http://127.0.0.1:11434/v1",
        model: "new-model",
        apiKey: undefined,
      },
    }),
  );
  expect(await screen.findByText("new-model")).toBeInTheDocument();
  await user.click(
    screen.getByRole("button", { name: "customApps.deleteProvider" }),
  );
  expect(invoke).not.toHaveBeenCalledWith(
    "delete_custom_provider",
    expect.anything(),
  );
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "common.confirm",
    }),
  );
  await waitFor(() =>
    expect(screen.queryByText("new-model")).not.toBeInTheDocument(),
  );
  await user.click(
    screen.getByRole("button", { name: "customApps.deleteApp" }),
  );
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "common.confirm",
    }),
  );
  expect(
    await screen.findByRole("button", { name: "customApps.addApp" }),
  ).toBeInTheDocument();
});

it.each(["::ffff:127.0.0.1", "[::ffff:7f00:1]", "::1"])(
  "advertises the mapped or IPv6 loopback endpoint %s",
  async (address) => {
    listener.address = address;
    invoke.mockImplementation(async (command) => {
      if (command === undefined) return;
      if (command === "list_custom_apps")
        return [{ id: "notes", name: "Notes" }];
      if (command === "list_custom_providers")
        return { providers: [], selected: null };
      throw new Error(`Unexpected Tauri command: ${command}`);
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <CustomAppsPage appId="notes" onSelectApp={vi.fn()} />
      </QueryClientProvider>,
    );
    const endpoint = await screen.findByRole("textbox", {
      name: "customApps.endpoint",
    });
    const host = address.startsWith("[") ? address : `[${address}]`;
    expect(endpoint).toHaveValue(`http://${host}:15721/apps/notes/v1`);
  },
);
