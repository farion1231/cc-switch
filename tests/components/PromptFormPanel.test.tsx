import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import PromptFormPanel from "@/components/prompts/PromptFormPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    onClose,
    footer,
    children,
  }: {
    onClose: () => void;
    footer?: React.ReactNode;
    children: React.ReactNode;
  }) => (
    <div>
      <button type="button" onClick={onClose}>
        panel-close
      </button>
      {children}
      {footer}
    </div>
  ),
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({
    value,
    onChange,
    readOnly,
  }: {
    value: string;
    onChange: (value: string) => void;
    readOnly?: boolean;
  }) => (
    <textarea
      aria-label="markdown-editor"
      value={value}
      disabled={readOnly}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

describe("PromptFormPanel", () => {
  beforeEach(() => {
    document.documentElement.classList.remove("dark");
  });

  it("submits once and refuses to close while saving", async () => {
    let resolveSave!: () => void;
    const onSave = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSave = resolve;
        }),
    );
    const onClose = vi.fn();
    render(
      <PromptFormPanel appId="claude" onSave={onSave} onClose={onClose} />,
    );

    const nameInput = screen.getByLabelText("prompts.name");
    fireEvent.change(nameInput, { target: { value: "My Prompt" } });
    const saveButton = screen.getByRole("button", { name: "common.save" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);
    fireEvent.click(screen.getByRole("button", { name: "panel-close" }));

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
    expect(nameInput).toBeDisabled();
    expect(screen.getByLabelText("markdown-editor")).toBeDisabled();

    await act(async () => {
      resolveSave();
      await Promise.resolve();
    });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("stays open when the parent write lock rejects a save", async () => {
    const onSave = vi.fn().mockResolvedValue(false);
    const onClose = vi.fn();
    render(<PromptFormPanel appId="codex" onSave={onSave} onClose={onClose} />);

    fireEvent.change(screen.getByLabelText("prompts.name"), {
      target: { value: "Codex Prompt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("prompts.name")).toBeEnabled();
  });
});
