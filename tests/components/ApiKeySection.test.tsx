import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiKeySection } from "@/components/providers/forms/shared/ApiKeySection";

describe("ApiKeySection", () => {
  it("selects one key and adds a new active key", () => {
    const onChange = vi.fn();
    const onApiKeysChange = vi.fn();
    render(
      <ApiKeySection
        value="first"
        onChange={onChange}
        apiKeys={["first", "second"]}
        onApiKeysChange={onApiKeysChange}
        shouldShowLink={false}
        websiteUrl=""
      />,
    );

    fireEvent.click(screen.getAllByRole("radio")[1]);
    expect(onChange).toHaveBeenLastCalledWith("second");

    fireEvent.click(screen.getByRole("button", { name: /新增 API Key/ }));
    expect(onApiKeysChange).toHaveBeenLastCalledWith(["first", "second", ""]);
    expect(onChange).toHaveBeenLastCalledWith("");
  });
});
