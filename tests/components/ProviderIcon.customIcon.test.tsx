import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { pngDataUri } from "../fixtures/customIcon";

describe("ProviderIcon custom icons", () => {
  it("renders a valid custom icon as an image", () => {
    render(<ProviderIcon icon={pngDataUri(96, 96)} name="Acme Relay" />);
    expect(screen.getByRole("img")).toHaveAttribute("src", pngDataUri(96, 96));
  });

  it("falls back to initials for icons that fail the render-time checks", () => {
    render(<ProviderIcon icon={pngDataUri(50000, 50000)} name="Acme Relay" />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("AR")).toBeInTheDocument();
  });

  it("falls back to initials when the image cannot be decoded", () => {
    render(<ProviderIcon icon={pngDataUri(96, 96)} name="Acme Relay" />);
    fireEvent.error(screen.getByRole("img"));
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText("AR")).toBeInTheDocument();
  });
});
