import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PiLoginProviderCard } from "@/components/providers/PiLoginProviderCard";

describe("PiLoginProviderCard", () => {
  it("shows the login source and no provider mutation actions", () => {
    render(
      <PiLoginProviderCard provider={{ id: "deepseek", name: "DeepSeek" }} />,
    );

    expect(
      screen.getByRole("heading", { name: "DeepSeek" }),
    ).toBeInTheDocument();
    expect(screen.getByText("pi.login.configured")).toBeInTheDocument();
    expect(screen.getAllByText("pi.login.managedHint")).not.toHaveLength(0);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(
      screen.queryByText("provider.notConfigured"),
    ).not.toBeInTheDocument();
  });
});
