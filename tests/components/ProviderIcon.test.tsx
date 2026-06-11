import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderIcon } from "@/components/ProviderIcon";

describe("ProviderIcon", () => {
  it("renders the Antigravity image asset", () => {
    render(<ProviderIcon icon="antigravity" name="Antigravity 2.0" />);

    expect(
      screen.getByRole("img", { name: "Antigravity 2.0" }),
    ).toHaveAttribute(
      "src",
      expect.stringContaining("antigravity"),
    );
  });
});
