import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import React from "react";
import { ProviderIcon } from "@/components/ProviderIcon";

describe("ProviderIcon", () => {
  it("renders correct fallback initials for normal names", () => {
    render(<ProviderIcon name="Claude Code" />);
    expect(screen.getByText("CC")).toBeInTheDocument();
  });

  it("handles leading and multiple consecutive whitespaces without undefined initials", () => {
    render(<ProviderIcon name="  Custom   Provider  " />);
    expect(screen.getByText("CP")).toBeInTheDocument();
    expect(screen.queryByText(/undefined/i)).not.toBeInTheDocument();
  });

  it("handles single word names correctly", () => {
    render(<ProviderIcon name="OpenAI" />);
    expect(screen.getByText("O")).toBeInTheDocument();
  });

  it("handles empty names gracefully", () => {
    render(<ProviderIcon name="   " />);
    expect(screen.getByText("?")).toBeInTheDocument();
  });
});
