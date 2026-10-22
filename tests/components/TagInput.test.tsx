import { useState } from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TagInput } from "@/components/ui/tag-input";

function TagInputHarness({
  initialTags = ["web", "docs"],
  suggestions = ["web", "docs", "tools", "database"],
  disabled = false,
}: {
  initialTags?: string[];
  suggestions?: string[];
  disabled?: boolean;
}) {
  const [tags, setTags] = useState(initialTags);
  return (
    <TagInput
      value={tags}
      onChange={setTags}
      suggestions={suggestions}
      disabled={disabled}
      placeholder="Add tag"
    />
  );
}

describe("TagInput", () => {
  it("renders initial tags as chips", () => {
    render(<TagInputHarness initialTags={["search", "ai"]} />);
    expect(screen.getByText("search")).toBeInTheDocument();
    expect(screen.getByText("ai")).toBeInTheDocument();
  });

  it("adds a new tag when pressing Enter", async () => {
    render(<TagInputHarness initialTags={["tools"]} />);
    const input = screen.getByPlaceholderText("");
    await userEvent.type(input, "developer{enter}");
    expect(screen.getByText("developer")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("adds a new tag when typing comma", async () => {
    render(<TagInputHarness initialTags={["tools"]} />);
    const input = screen.getByPlaceholderText("");
    await userEvent.type(input, "utility,");
    expect(screen.getByText("utility")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("prevents duplicate tags", async () => {
    render(<TagInputHarness initialTags={["alpha"]} />);
    const input = screen.getByPlaceholderText("");
    await userEvent.type(input, "alpha{enter}");
    expect(screen.getAllByText("alpha")).toHaveLength(1);
  });

  it("removes a tag when clicking the remove button", async () => {
    render(<TagInputHarness initialTags={["removable", "keep"]} />);
    const removeBtn = screen.getByLabelText("Remove tag removable");
    await userEvent.click(removeBtn);
    expect(screen.queryByText("removable")).not.toBeInTheDocument();
    expect(screen.getByText("keep")).toBeInTheDocument();
  });

  it("removes the last tag on Backspace when input is empty", async () => {
    render(<TagInputHarness initialTags={["first", "second"]} />);
    const input = screen.getByPlaceholderText("");
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(screen.getByText("first")).toBeInTheDocument();
    expect(screen.queryByText("second")).not.toBeInTheDocument();
  });

  it("shows suggestions and allows selecting via click", async () => {
    render(
      <TagInputHarness
        initialTags={[]}
        suggestions={["database", "design", "deploy"]}
      />,
    );
    const input = screen.getByPlaceholderText("Add tag");
    await userEvent.type(input, "data");
    const option = screen.getByRole("option", { name: /database/i });
    expect(option).toBeInTheDocument();
    fireEvent.mouseDown(option);
    expect(screen.getByText("database")).toBeInTheDocument();
  });

  it("navigates suggestions via ArrowDown and selects via Enter", async () => {
    render(
      <TagInputHarness initialTags={[]} suggestions={["database", "deploy"]} />,
    );
    const input = screen.getByPlaceholderText("Add tag");
    await userEvent.type(input, "de");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByText("deploy")).toBeInTheDocument();
  });

  it("disables input and removes when disabled prop is true", () => {
    render(<TagInputHarness initialTags={["locked"]} disabled={true} />);
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(
      screen.queryByLabelText("Remove tag locked"),
    ).not.toBeInTheDocument();
  });
});
