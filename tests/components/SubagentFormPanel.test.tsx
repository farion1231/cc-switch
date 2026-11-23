import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import SubagentFormPanel from "@/components/subagents/SubagentFormPanel";
import { discardUnsavedChanges, hasUnsavedChanges } from "@/lib/unsavedChanges";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.value === undefined ? key : `${key}:${String(options.value)}`,
    i18n: { language: "en" },
  }),
}));

vi.mock("@/components/MarkdownEditor", () => ({
  default: ({
    value,
    onChange,
    placeholder,
    readOnly,
  }: {
    value: string;
    onChange?: (value: string) => void;
    placeholder?: string;
    readOnly?: boolean;
  }) => (
    <textarea
      aria-label={placeholder ?? "source"}
      value={value}
      readOnly={readOnly}
      onChange={(event) => onChange?.(event.target.value)}
    />
  ),
}));

vi.mock("@/components/subagents/SubagentModelSelect", () => ({
  SubagentModelSelect: ({ disabled }: { disabled?: boolean }) => (
    <button type="button" disabled={disabled}>
      model
    </button>
  ),
}));

vi.mock("@/components/subagents/SubagentToolsSelect", () => ({
  SubagentToolsSelect: ({ disabled }: { disabled?: boolean }) => (
    <button type="button" disabled={disabled}>
      tools
    </button>
  ),
}));

// Claude Code `/agents` 生成的写法：严格 YAML 解析不了，Claude Code 和后端都认
const AGENTS_GENERATED = `---
name: api-designer
description: Use this agent when designing APIs. Examples: <example>Context: The user adds a resource. user: "Add orders" assistant: "Using api-designer"</example>
model: sonnet
---

You design APIs.
`;

function renderPanel(onSave = vi.fn().mockResolvedValue({ status: "saved" })) {
  const onClose = vi.fn();
  render(
    <SubagentFormPanel
      subagent={{
        id: "a1",
        name: "api-designer",
        fileName: "api-designer",
        description: "Use this agent when designing APIs.",
        content: AGENTS_GENERATED,
        enabled: true,
        source: "imported",
        status: "synced",
        filePath: "/home/u/.claude/agents/api-designer.md",
      }}
      initialContent={AGENTS_GENERATED}
      initialEnabled
      takenNames={new Set()}
      onSave={onSave}
      onClose={onClose}
    />,
  );
  return { onSave, onClose };
}

describe("SubagentFormPanel with /agents generated frontmatter", () => {
  afterEach(() => discardUnsavedChanges());

  it("opens in form mode with read-only fields and saves body edits verbatim", async () => {
    const user = userEvent.setup();
    const { onSave, onClose } = renderPanel();

    expect(screen.getByText("subagents.form.looseTitle")).toBeInTheDocument();
    const name = screen.getByLabelText(/subagents\.form\.name/);
    expect(name).toHaveValue("api-designer");
    expect(name).toHaveAttribute("readonly");
    expect(screen.getByLabelText(/subagents\.form\.description/)).toHaveValue(
      'Use this agent when designing APIs. Examples: <example>Context: The user adds a resource. user: "Add orders" assistant: "Using api-designer"</example>',
    );
    expect(screen.getByRole("button", { name: "model" })).toBeDisabled();

    const body = screen.getByLabelText("subagents.form.bodyPlaceholder");
    fireEvent.change(body, { target: { value: "You design REST APIs.\n" } });
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );

    expect(onSave).toHaveBeenCalledWith({
      id: "a1",
      fileName: "api-designer",
      content: AGENTS_GENERATED.replace(
        "You design APIs.\n",
        "You design REST APIs.\n",
      ),
      enabled: true,
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("saves straight from source mode and leaves validation to the backend", async () => {
    const user = userEvent.setup();
    const { onSave } = renderPanel();

    await user.click(
      screen.getByRole("button", { name: "subagents.form.viewSource" }),
    );
    const source = screen.getByLabelText("source");
    const edited = AGENTS_GENERATED.replace("model: sonnet", "model: opus");
    fireEvent.change(source, { target: { value: edited } });
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ content: edited }),
    );
  });

  it("registers drafts so leaving the page asks first", () => {
    renderPanel();
    expect(hasUnsavedChanges()).toBe(false);
    fireEvent.input(screen.getByLabelText("subagents.form.bodyPlaceholder"), {
      target: { value: "draft" },
    });
    expect(hasUnsavedChanges()).toBe(true);
  });
});

describe("SubagentFormPanel effort field", () => {
  beforeEach(() => {
    // Radix Select 打开时会滚动到选中项，jsdom 没有 scrollIntoView
    Element.prototype.scrollIntoView ??= vi.fn();
  });
  afterEach(() => discardUnsavedChanges());

  function renderEffortForm(initialContent: string, isNew = false) {
    const onSave = vi.fn().mockResolvedValue({ status: "saved" });
    const onClose = vi.fn();
    render(
      <SubagentFormPanel
        subagent={
          isNew
            ? undefined
            : {
                id: "a1",
                name: "api-designer",
                fileName: "api-designer",
                description: "Desc",
                content: initialContent,
                enabled: true,
                source: "local",
                status: "synced",
                filePath: "/home/u/.claude/agents/api-designer.md",
              }
        }
        initialContent={initialContent}
        initialEnabled
        takenNames={new Set()}
        onSave={onSave}
        onClose={onClose}
      />,
    );
    return { onSave, onClose };
  }

  it("lists effort options, updates frontmatter, marks draft dirty, and removes effort when unset", async () => {
    const user = userEvent.setup();
    const { onSave } = renderEffortForm(
      '---\nname: ""\ndescription: ""\n---\n\n',
      true,
    );

    const trigger = screen.getByRole("combobox", {
      name: "subagents.form.effort",
    });
    expect(trigger).toHaveTextContent("subagents.effort.unset");
    expect(trigger).toHaveAccessibleDescription("subagents.effort.hint");

    await user.click(trigger);
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "subagents.effort.unset",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);

    expect(hasUnsavedChanges()).toBe(false);
    await user.click(screen.getByRole("option", { name: "high" }));
    expect(trigger).toHaveTextContent("high");
    expect(hasUnsavedChanges()).toBe(true);

    fireEvent.change(screen.getByLabelText(/subagents\.form\.name/), {
      target: { value: "code-reviewer" },
    });
    fireEvent.change(screen.getByLabelText(/subagents\.form\.description/), {
      target: { value: "Reviews diffs" },
    });
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        content:
          '---\nname: "code-reviewer"\ndescription: "Reviews diffs"\neffort: high\n---\n\n',
      }),
    );

    // Selecting unset removes the effort key cleanly
    await user.click(trigger);
    await user.click(
      screen.getByRole("option", { name: "subagents.effort.unset" }),
    );
    expect(trigger).toHaveTextContent("subagents.effort.unset");
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenLastCalledWith(
      expect.objectContaining({
        content:
          '---\nname: "code-reviewer"\ndescription: "Reviews diffs"\n---\n\n',
      }),
    );
  });

  it("preserves custom effort values unless explicitly changed", async () => {
    const user = userEvent.setup();
    const initial =
      "---\nname: api-designer\ndescription: Desc\neffort: custom-tier # keep\n---\n\nPrompt.\n";
    const { onSave } = renderEffortForm(initial);

    const trigger = screen.getByRole("combobox", {
      name: "subagents.form.effort",
    });
    expect(trigger).toHaveTextContent(
      "subagents.effort.customOption:custom-tier",
    );

    fireEvent.change(screen.getByLabelText(/subagents\.form\.description/), {
      target: { value: "Updated desc" },
    });
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        content:
          "---\nname: api-designer\ndescription: Updated desc\neffort: custom-tier # keep\n---\n\nPrompt.\n",
      }),
    );

    // Explicitly switching to xhigh updates effort while keeping comment
    await user.click(trigger);
    await user.click(screen.getByRole("option", { name: "xhigh" }));
    expect(trigger).toHaveTextContent("xhigh");
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenLastCalledWith(
      expect.objectContaining({
        content:
          "---\nname: api-designer\ndescription: Updated desc\neffort: xhigh # keep\n---\n\nPrompt.\n",
      }),
    );

    // Explicitly switching to unset deletes effort
    await user.click(trigger);
    await user.click(
      screen.getByRole("option", { name: "subagents.effort.unset" }),
    );
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenLastCalledWith(
      expect.objectContaining({
        content:
          "---\nname: api-designer\ndescription: Updated desc\n---\n\nPrompt.\n",
      }),
    );
  });

  it("shows effort read-only on /agents loose frontmatter and preserves it on body save", async () => {
    const user = userEvent.setup();
    const looseWithEffort = AGENTS_GENERATED.replace(
      "model: sonnet\n",
      "model: sonnet\neffort: xhigh\n",
    );
    const { onSave } = renderEffortForm(looseWithEffort);

    const trigger = screen.getByRole("combobox", {
      name: "subagents.form.effort",
    });
    expect(trigger).toBeDisabled();
    expect(trigger).toHaveTextContent("xhigh");

    fireEvent.change(screen.getByLabelText("subagents.form.bodyPlaceholder"), {
      target: { value: "Updated prompt.\n" },
    });
    await user.click(
      screen.getByRole("button", { name: "subagents.form.saveAndWrite" }),
    );
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        content: looseWithEffort.replace(
          "You design APIs.\n",
          "Updated prompt.\n",
        ),
      }),
    );
  });
});
