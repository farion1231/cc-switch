import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";
import { PiAgentPanel } from "@/components/pi/PiAgentPanel";
import { piApi } from "@/lib/api";
import type { PiProvidersMap } from "@/types/pi";

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({ name }: { name: string }) => <span>{name}</span>,
}));

vi.mock("@/lib/api", () => ({
  piApi: {
    listProviders: vi.fn(),
    previewProviderPatch: vi.fn(),
    applyProviderPatch: vi.fn(),
    deleteProvider: vi.fn(),
    testConnectivity: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (options && typeof options === "object" && "id" in options) {
        return `${key}:${String(options.id)}`;
      }
      return key;
    },
    i18n: { language: "zh" },
  }),
  initReactI18next: { type: "3rdParty" as const, init: () => {} },
}));

const mockedListProviders = vi.mocked(piApi.listProviders);
const mockedPreview = vi.mocked(piApi.previewProviderPatch);
const mockedApply = vi.mocked(piApi.applyProviderPatch);
const mockedDelete = vi.mocked(piApi.deleteProvider);

describe("Pi Agent app entry", () => {
  it("renders Pi Agent as an app option", () => {
    render(<AppSwitcher activeApp="claude" onSwitch={vi.fn()} />);

    expect(screen.getAllByText("Pi Agent").length).toBeGreaterThan(0);
  });
});

describe("PiAgentPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedListProviders.mockResolvedValue({});
  });

  it("renders the provider list and opens the editor when edit is clicked", async () => {
    const providers: PiProvidersMap = {
      myprovider: {
        baseUrl: "https://api.example.com/v1",
        api: "openai-completions",
        apiKey: "$API_KEY",
        models: [{ id: "model-1", name: "Model 1" }],
      },
    };
    mockedListProviders.mockResolvedValueOnce(providers);

    render(<PiAgentPanel />);

    await waitFor(() => {
      expect(screen.getByText("myprovider")).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTitle("pi.list.edit"));
    });

    await waitFor(() => {
      expect(screen.getByText("pi.editor.title")).toBeInTheDocument();
    });

    expect(screen.getByDisplayValue("myprovider")).toBeInTheDocument();
  });

  it("saves a provider with one click using preview then apply", async () => {
    mockedPreview.mockResolvedValueOnce({
      currentFileHash: "hash-1",
      nextModelsJson: { providers: { myprovider: {} } },
      summary: ["Upsert Pi provider myprovider"],
    });
    mockedApply.mockResolvedValueOnce({
      fileHash: "hash-2",
      modelsJson: { providers: { myprovider: {} } },
      backupPath: "/tmp/models.json.bak",
    });

    const ref = { current: null as null | { openAdd: () => void } };
    render(<PiAgentPanel ref={ref} />);

    await waitFor(() => {
      expect(screen.getByText("pi.list.emptyHint")).toBeInTheDocument();
    });

    await act(async () => {
      ref.current?.openAdd();
    });

    await waitFor(() => {
      expect(screen.getByText("pi.editor.title")).toBeInTheDocument();
    });

    // Fill providerId
    await act(async () => {
      fireEvent.change(screen.getByPlaceholderText("my-openai"), {
        target: { value: "myprovider" },
      });
    });
    // Fill baseUrl so the config JSON preview is non-empty
    await act(async () => {
      fireEvent.change(
        screen.getByPlaceholderText("https://api.example.com/v1"),
        {
          target: { value: "https://api.example.com/v1" },
        },
      );
    });

    await act(async () => {
      fireEvent.click(screen.getByText("common.save"));
    });

    await waitFor(() => {
      expect(mockedPreview).toHaveBeenCalled();
      expect(mockedApply).toHaveBeenCalledWith(
        expect.objectContaining({ providerId: "myprovider" }),
        "hash-1",
      );
    });

    // Returns to list view after save
    await waitFor(() => {
      expect(screen.queryByText("pi.editor.title")).not.toBeInTheDocument();
    });
  });

  it("deletes a provider after confirmation", async () => {
    const providers: PiProvidersMap = {
      todelete: {
        baseUrl: "https://api.example.com/v1",
        api: "openai-completions",
        apiKey: "$API_KEY",
        models: [{ id: "model-1" }],
      },
    };
    mockedListProviders.mockResolvedValueOnce(providers);
    mockedPreview.mockResolvedValueOnce({
      currentFileHash: "hash-1",
      nextModelsJson: { providers: {} },
      summary: ["Delete Pi provider todelete"],
    });
    mockedDelete.mockResolvedValueOnce({
      fileHash: "hash-2",
      modelsJson: { providers: {} },
      backupPath: "/tmp/models.json.bak",
    });

    render(<PiAgentPanel />);

    await waitFor(() => {
      expect(screen.getByText("todelete")).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByTitle("pi.list.delete"));
    });

    await waitFor(() => {
      expect(screen.getByText("pi.deleteConfirm.title")).toBeInTheDocument();
    });

    await act(async () => {
      fireEvent.click(screen.getByText("common.delete"));
    });

    await waitFor(() => {
      expect(mockedPreview).toHaveBeenCalled();
      expect(mockedDelete).toHaveBeenCalledWith("todelete", "hash-1");
    });
  });
});
