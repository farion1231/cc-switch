import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PiAgentPanel } from "@/components/pi/PiAgentPanel";
import { piApi } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  piApi: {
    listProviders: vi.fn().mockResolvedValue({}),
    previewProviderPatch: vi.fn().mockResolvedValue({
      currentFileHash: "hash-1",
      nextModelsJson: { providers: { "my-provider": {} } },
      summary: ['Upsert provider "my-provider"'],
    }),
    applyProviderPatch: vi.fn().mockResolvedValue({
      fileHash: "hash-2",
      modelsJson: { providers: { "my-provider": {} } },
      backupPath: "/tmp/backup.json",
    }),
    deleteProvider: vi.fn().mockResolvedValue({
      fileHash: "hash-2",
      modelsJson: { providers: {} },
      backupPath: "/tmp/backup.json",
    }),
  },
}));

describe("PiAgentPanel", () => {
  it("requires preview before applying provider changes", async () => {
    const { rerender } = render(<PiAgentPanel addTrigger={0} />);

    rerender(<PiAgentPanel addTrigger={1} />);

    expect(await screen.findByText("Add Provider")).toBeInTheDocument();

    fireEvent.click(screen.getByText("OpenAI-compatible"));
    fireEvent.change(screen.getByLabelText(/Provider ID/i), {
      target: { value: "my-provider" },
    });
    fireEvent.change(screen.getByLabelText(/Base URL/i), {
      target: { value: "https://api.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText(/Model ID/i), {
      target: { value: "model-a" },
    });

    expect(
      screen.queryByText(/Apply to models\.json/i),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByText(/Preview & Review/i));

    await waitFor(() => expect(piApi.previewProviderPatch).toHaveBeenCalled());
    await screen.findByText("Review Changes");
    expect(screen.getAllByText(/Apply to models\.json/i)).toHaveLength(2);
    expect(screen.getAllByText(/Apply to models\.json/i)[0]).toBeEnabled();
  });
});
