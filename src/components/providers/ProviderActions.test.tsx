import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProviderActions } from "@/components/providers/ProviderActions";

function renderActions(
  props: Partial<Parameters<typeof ProviderActions>[0]> = {},
) {
  return render(
    <ProviderActions
      appId="claude"
      isCurrent={false}
      onSwitch={vi.fn()}
      onEdit={vi.fn()}
      onDuplicate={vi.fn()}
      onDelete={vi.fn()}
      {...props}
    />,
  );
}

describe("ProviderActions share button", () => {
  it("renders the share button and fires onShare on click", async () => {
    const onShare = vi.fn();
    renderActions({ onShare });

    const button = screen.getByTitle("复制分享链接");
    await userEvent.click(button);
    expect(onShare).toHaveBeenCalledTimes(1);
  });

  it("hides the share button when onShare is not provided", () => {
    renderActions();
    expect(screen.queryByTitle("复制分享链接")).toBeNull();
  });
});
