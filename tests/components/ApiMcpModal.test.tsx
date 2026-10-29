import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ApiMcpModal from "@/components/mcp/ApiMcpModal";

const mocks = vi.hoisted(() => ({
  preview: vi.fn(),
  build: vi.fn(),
  decode: vi.fn(),
  upsert: vi.fn(),
}));
vi.mock("@/lib/api/mcp", () => ({
  mcpApi: {
    previewApi: mocks.preview,
    buildApiSpec: mocks.build,
    decodeApiSpec: mocks.decode,
  },
}));
vi.mock("@/hooks/useMcp", () => ({
  useUpsertMcpServer: () => ({ mutateAsync: mocks.upsert }),
}));
const preview = {
  title: "Weather",
  tools: [
    {
      name: "weather",
      method: "GET",
      url: "https://api.example.com/weather",
      description: "Weather",
      inputSchema: { type: "object", properties: {} },
    },
    {
      name: "deleteWeather",
      method: "DELETE",
      url: "https://api.example.com/weather",
      description: "Delete",
      inputSchema: { type: "object", properties: {} },
    },
  ],
  skipped: [],
};

describe("API MCP creation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.preview.mockResolvedValue(preview);
    mocks.build.mockResolvedValue({
      command: "cc-switch",
      args: ["--mcp-api"],
      env: {},
    });
    mocks.upsert.mockResolvedValue(undefined);
  });
  const renderModal = () =>
    render(
      <ApiMcpModal
        existingServers={{}}
        visibleAppIds={["claude", "codex"]}
        onClose={vi.fn()}
      />,
    );
  it("previews a document and saves only explicitly selected operations with the selected apps", async () => {
    renderModal();
    fireEvent.change(screen.getByLabelText("mcpApi.documentUrl"), {
      target: { value: "https://api.example.com/openapi.json" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    expect(screen.getByRole("checkbox", { name: "weather" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "deleteWeather" }),
    ).not.toBeChecked();
    fireEvent.change(screen.getByLabelText("mcpApi.serverId"), {
      target: { value: "weather-api" },
    });
    await userEvent.click(screen.getByRole("checkbox", { name: "Codex" }));
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(mocks.upsert).toHaveBeenCalledOnce());
    expect(mocks.build.mock.calls[0][0].tools.map((x: any) => x.name)).toEqual([
      "weather",
    ]);
    expect(mocks.upsert.mock.calls[0][0]).toMatchObject({
      id: "weather-api",
      apps: { claude: true, codex: false },
    });
  });
  it("supports a manual REST definition and never saves after a preview failure", async () => {
    renderModal();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.manual" }),
    );
    fireEvent.change(screen.getByLabelText("mcpApi.toolName"), {
      target: { value: "weather" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.endpoint"), {
      target: { value: "https://api.example.com/weather" },
    });
    mocks.preview.mockRejectedValueOnce(new Error("invalid schema"));
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByRole("alert");
    expect(mocks.upsert).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    const request = mocks.preview.mock.calls[1][0];
    expect(JSON.parse(request.document).paths["/weather"].get.operationId).toBe(
      "weather",
    );
  });
  it("invalidates old preview as soon as the source changes", async () => {
    renderModal();
    fireEvent.change(screen.getByLabelText("mcpApi.documentUrl"), {
      target: { value: "https://api.example.com/openapi.json" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    fireEvent.change(screen.getByLabelText("mcpApi.documentUrl"), {
      target: { value: "https://other.example.com/openapi.json" },
    });
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  });

  it("focuses save errors so feedback stays visible in a scrolled form", async () => {
    renderModal();
    fireEvent.change(screen.getByLabelText("mcpApi.documentUrl"), {
      target: { value: "https://api.example.com/openapi.json" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    fireEvent.change(screen.getByLabelText("mcpApi.headers"), {
      target: { value: '{"Authorization":7}' },
    });
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    const error = await screen.findByRole("alert");
    expect(error).toHaveFocus();
    expect(mocks.upsert).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(error).toHaveFocus();
  });

  it("preserves repeated query values when importing a manual URL", async () => {
    renderModal();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.manual" }),
    );
    fireEvent.change(screen.getByLabelText("mcpApi.toolName"), {
      target: { value: "items" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.endpoint"), {
      target: { value: "https://api.example.com/items?tag=a&tag=b" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    const operation = JSON.parse(mocks.preview.mock.calls[0][0].document).paths[
      "/items"
    ].get;
    expect(operation.parameters).toEqual([
      {
        in: "query",
        name: "tag",
        schema: {
          type: "array",
          items: { type: "string" },
          default: ["a", "b"],
        },
      },
    ]);
  });

  it("preserves URL escapes while recognizing manual path placeholders", async () => {
    renderModal();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.manual" }),
    );
    fireEvent.change(screen.getByLabelText("mcpApi.toolName"), {
      target: { value: "item" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.endpoint"), {
      target: { value: "https://api.example.com/files/a%252Fb/100%25/{id}" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    const paths = JSON.parse(mocks.preview.mock.calls[0][0].document).paths;
    expect(Object.keys(paths)).toEqual(["/files/a%252Fb/100%25/{id}"]);
    expect(paths["/files/a%252Fb/100%25/{id}"].get.parameters).toContainEqual({
      in: "path",
      name: "id",
      required: true,
      schema: { type: "string" },
    });
  });

  it("rejects embedded URL credentials instead of silently dropping them", async () => {
    renderModal();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.manual" }),
    );
    fireEvent.change(screen.getByLabelText("mcpApi.toolName"), {
      target: { value: "items" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.endpoint"), {
      target: { value: "https://example-user@example.com/items" },
    });
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByRole("alert");
    expect(mocks.preview).not.toHaveBeenCalled();
  });

  it("marks a POST search as a read-only business query when explicitly selected", async () => {
    renderModal();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.manual" }),
    );
    fireEvent.change(screen.getByLabelText("mcpApi.toolName"), {
      target: { value: "search" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.endpoint"), {
      target: { value: "https://api.example.com/search" },
    });
    fireEvent.change(screen.getByLabelText("mcpApi.method"), {
      target: { value: "POST" },
    });
    const readOnly = screen.getByRole("checkbox", { name: "mcpApi.readOnly" });
    expect(readOnly).not.toBeChecked();
    await userEvent.click(readOnly);
    await userEvent.click(
      screen.getByRole("button", { name: "mcpApi.preview" }),
    );
    await screen.findByText("GET https://api.example.com/weather");
    expect(
      JSON.parse(mocks.preview.mock.calls[0][0].document).paths["/search"].post[
        "x-mcp-read-only"
      ],
    ).toBe(true);
  });
});
