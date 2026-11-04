import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { pngDataUri } from "../fixtures/customIcon";

describe("ProviderIcon custom icons", () => {
  it("renders a valid custom icon as an image", () => {
    render(<ProviderIcon icon={pngDataUri(96, 96)} name="Acme Relay" />);
    expect(screen.getByRole("img")).toHaveAttribute("src", pngDataUri(96, 96));
  });

  it("uses initials instead of loading unsupported or oversized icons", () => {
    render(<ProviderIcon icon={pngDataUri(50000, 50000)} name="Acme Relay" />);
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText("AR")).toBeInTheDocument();
  });

  it("falls back on decoding errors and can render a subsequent icon", () => {
    const { rerender } = render(
      <ProviderIcon icon={pngDataUri(96, 96)} name="Acme Relay" />,
    );
    fireEvent.error(screen.getByRole("img"));
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByText("AR")).toBeInTheDocument();
    rerender(<ProviderIcon icon={pngDataUri(64, 64)} name="Acme Relay" />);
    expect(screen.getByRole("img")).toHaveAttribute("src", pngDataUri(64, 64));
  });
});
