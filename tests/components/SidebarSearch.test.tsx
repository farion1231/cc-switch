import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  SidebarSearchInput,
  SidebarSearchResults,
  useSidebarSearch,
  type SidebarSearchOptions,
} from "@/components/shell/SidebarSearch";

function Harness(props: SidebarSearchOptions) {
  const search = useSidebarSearch(props);
  return (
    <>
      <SidebarSearchInput search={search} listId="results" />
      {search.searching ? (
        <SidebarSearchResults search={search} listId="results" />
      ) : (
        <p>directory</p>
      )}
    </>
  );
}

function renderSearch() {
  const props = {
    visibleApps: { claude: true, codex: true, opencode: false } as never,
    onSelectApp: vi.fn(),
    onSelectPage: vi.fn(),
    onOpenSettings: vi.fn(),
  };
  render(<Harness {...props} />);
  return props;
}

describe("SidebarSearch", () => {
  it("filters in place and jumps to the first match on Enter", async () => {
    const user = userEvent.setup();
    const props = renderSearch();

    expect(screen.getByText("directory")).toBeInTheDocument();
    await user.type(screen.getByRole("combobox"), "codex");
    expect(screen.queryByText("directory")).not.toBeInTheDocument();
    await user.keyboard("{Enter}");

    expect(props.onSelectApp).toHaveBeenCalledWith("codex");
    expect(screen.getByRole("combobox")).toHaveValue("");
  });

  it("moves with arrow keys, leaves out hidden apps, and clears on Escape", async () => {
    const user = userEvent.setup();
    const props = renderSearch();
    const input = screen.getByRole("combobox");

    await user.type(input, "open");
    expect(screen.queryByText("OpenCode")).not.toBeInTheDocument();

    await user.clear(input);
    await user.type(input, "s");
    await user.keyboard("{ArrowDown}");
    const options = screen.getAllByRole("option");
    expect(options[1]).toHaveAttribute("aria-selected", "true");

    await user.keyboard("{Escape}");
    expect(input).toHaveValue("");
    expect(screen.getByText("directory")).toBeInTheDocument();
    expect(props.onSelectApp).not.toHaveBeenCalled();
  });

  it("opens pages and settings by click", async () => {
    const user = userEvent.setup();
    const props = renderSearch();

    await user.type(screen.getByRole("combobox"), "mcp");
    await user.click(screen.getByRole("option", { name: "MCP" }));
    expect(props.onSelectPage).toHaveBeenCalledWith("mcp");

    await user.type(screen.getByRole("combobox"), "routing");
    await user.click(screen.getAllByRole("option")[0]);
    expect(props.onOpenSettings).toHaveBeenCalledWith("routing");
  });
});
