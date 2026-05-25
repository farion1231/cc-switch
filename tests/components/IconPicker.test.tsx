import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { IconPicker } from "@/components/IconPicker";

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: ({ icon, name }: { icon?: string; name: string }) => (
    <span data-icon={icon}>{name}</span>
  ),
}));

vi.mock("@/icons/extracted", () => ({
  iconList: ["claude", "openai"],
}));

vi.mock("@/icons/extracted/metadata", () => ({
  getIconMetadata: (name: string) => ({
    name,
    displayName: name,
    defaultColor: "currentColor",
  }),
  searchIcons: (query: string) =>
    ["claude", "openai"].filter((name) => name.includes(query)),
}));

describe("IconPicker", () => {
  it("commits a pasted custom icon URL before the picker unmounts", () => {
    const onValueChange = vi.fn();
    const customIconUrl = "https://cdn.example.com/provider-icon.png";
    const { unmount } = render(
      <IconPicker value="openai" onValueChange={onValueChange} />,
    );

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: customIconUrl },
    });
    unmount();

    expect(onValueChange).toHaveBeenCalledTimes(1);
    expect(onValueChange).toHaveBeenCalledWith(customIconUrl);
  });

  it("clears an existing custom icon URL before the picker unmounts", () => {
    const onValueChange = vi.fn();
    const customIconUrl = "https://cdn.example.com/provider-icon.png";
    const { unmount } = render(
      <IconPicker value={customIconUrl} onValueChange={onValueChange} />,
    );

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "" },
    });
    unmount();

    expect(onValueChange).toHaveBeenCalledTimes(1);
    expect(onValueChange).toHaveBeenCalledWith("");
  });
});
