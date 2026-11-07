import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { OmpLoginCard } from "@/components/providers/OmpLoginCard";
import type { OmpLogin } from "@/lib/api/omp";

const login: OmpLogin = {
  provider: "opencode-go",
  credentialTypes: ["apiKey"],
  total: 2,
  active: 1,
};

describe("OmpLoginCard", () => {
  it("shows a known provider, credential type, account count and management help without actions", () => {
    render(<OmpLoginCard login={login} />);
    expect(screen.getByRole("heading", { name: "OpenCode Go" })).toBeVisible();
    expect(screen.getByText("omp.login.apiKey")).toBeVisible();
    expect(screen.getByText("omp.login.accounts")).toBeVisible();
    expect(screen.getByText("omp.login.managedHint")).toBeInTheDocument();
    expect(screen.queryByText("omp.login.disabled")).not.toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "omp.login.chip" }),
    ).toBeVisible();
  });

  it("shows OAuth for OpenAI Codex and warns when all accounts are disabled", () => {
    render(
      <OmpLoginCard
        login={{
          ...login,
          provider: "openai-codex",
          credentialTypes: ["oauth"],
          active: 0,
        }}
      />,
    );
    expect(screen.getByRole("heading", { name: "OpenAI Codex" })).toBeVisible();
    expect(screen.getByText("omp.login.oauth")).toBeVisible();
    expect(screen.getByText("omp.login.disabled")).toBeVisible();
  });

  it("keeps an unknown provider id and credential type visible without a single-account count", () => {
    render(
      <OmpLoginCard
        login={{
          ...login,
          provider: "new-provider",
          credentialTypes: ["apiKey", "oauth", "unknown"],
          total: 1,
        }}
      />,
    );
    expect(screen.getByRole("heading", { name: "new-provider" })).toBeVisible();
    for (const type of ["apiKey", "oauth", "unknown"]) {
      expect(screen.getByText(`omp.login.${type}`)).toBeVisible();
    }
    expect(screen.queryByText("omp.login.accounts")).not.toBeInTheDocument();
  });
});
