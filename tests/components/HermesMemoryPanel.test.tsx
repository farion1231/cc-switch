import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/hooks/useHermes", () => ({
  useHermesMemory: (kind: string) => ({
    data: kind === "memory" ? "agent notes" : "user profile",
    isLoading: false,
  }),
  useHermesMemoryLimits: () => ({
    data: { memory: 2200, user: 1375, memoryEnabled: true, userEnabled: true },
  }),
  useOpenHermesWebUI: () => vi.fn(),
  useSaveHermesMemory: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useToggleHermesMemoryEnabled: () => ({ mutate: vi.fn(), isPending: false }),
}));
vi.mock("@/hooks/useDarkMode", () => ({ useDarkMode: () => false }));
vi.mock("@/components/MarkdownEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

import HermesMemoryPanel from "@/components/hermes/HermesMemoryPanel";

describe("HermesMemoryPanel", () => {
  it("switches memory files with secondary underline tabs and keeps unsaved edits", () => {
    render(<HermesMemoryPanel />);

    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "hermes.memory.agentTab",
      "hermes.memory.userTab",
    ]);
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("group")).not.toBeInTheDocument();

    const [agentEditor] = screen.getAllByLabelText("editor");
    fireEvent.change(agentEditor, { target: { value: "draft" } });

    fireEvent.click(tabs[1]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "hermes-memory-user",
    );

    fireEvent.click(tabs[0]);
    expect(
      screen.getAllByLabelText("editor", { selector: "textarea" })[0],
    ).toHaveValue("draft");
  });
});
