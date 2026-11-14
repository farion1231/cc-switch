import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { CustomAppsPage } from "@/components/custom-apps/CustomAppsPage";

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    status: { running: true, address: "127.0.0.1", port: 15721 },
  }),
}));

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

beforeEach(() => invoke.mockReset());

it("creates a profile, adds its provider and switches that profile", async () => {
  const user = userEvent.setup();
  const app = { id: "meetings", name: "Meeting summaries" };
  let created = false;
  let added = false;
  let selected = false;
  invoke.mockImplementation(async (command) => {
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
            ? [{ id: "local", name: "Local", model: "qwen" }]
            : [],
          selected: selected ? "local" : null,
        };
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
    screen.getByRole("textbox", { name: "customApps.appName" }),
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
});
