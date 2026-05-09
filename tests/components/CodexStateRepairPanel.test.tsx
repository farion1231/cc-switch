import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexStateRepairPanel } from "@/components/settings/CodexStateRepairPanel";
import type {
  CodexStateDiagnosis,
  CodexStateRepairResult,
} from "@/lib/api/providers";
import { server } from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const diagnosis: CodexStateDiagnosis = {
  configModelProvider: "openai",
  effectiveModelProvider: "shengsuanyun",
  authMode: "apikey",
  stateDbPath: "/tmp/codex-state.sqlite",
  providerCounts: [
    { modelProvider: "openai", count: 3 },
    { modelProvider: "shengsuanyun", count: 7 },
  ],
  configAuthMismatch: true,
  indexMismatch: true,
  inconsistent: true,
  repairableRows: 3,
};

describe("CodexStateRepairPanel", () => {
  const repairPayloads: unknown[] = [];

  beforeEach(() => {
    repairPayloads.length = 0;

    server.use(
      http.post(`${TAURI_ENDPOINT}/diagnose_codex_state`, () =>
        HttpResponse.json(diagnosis),
      ),
      http.post(`${TAURI_ENDPOINT}/repair_codex_state`, async ({ request }) => {
        const payload = await request.json();
        repairPayloads.push(payload);

        const dryRun = Boolean((payload as { dryRun?: boolean }).dryRun);
        const result: CodexStateRepairResult = {
          dryRun,
          targetModelProvider: "shengsuanyun",
          affectedRows: 3,
          backupPath: dryRun ? null : "/tmp/codex-state.sqlite.bak",
          diagnosisBefore: diagnosis,
          diagnosisAfter: { ...diagnosis, inconsistent: false },
        };

        return HttpResponse.json(result);
      }),
    );
  });

  it("renders inconsistent diagnosis details, repairable rows, and provider counts", async () => {
    render(<CodexStateRepairPanel />);

    expect(
      await screen.findByText("Codex state mismatch detected"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Config/auth mismatch:", { exact: false }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("openai").length).toBeGreaterThan(0);
    expect(screen.getAllByText("shengsuanyun").length).toBeGreaterThan(0);
    expect(screen.getByText("apikey")).toBeInTheDocument();
    expect(
      screen.getByText("Repairable rows:", { exact: false }),
    ).toBeInTheDocument();
    expect(screen.getByText("/tmp/codex-state.sqlite")).toBeInTheDocument();
    expect(screen.getByText("Thread index buckets")).toBeInTheDocument();
    expect(screen.getAllByText("3").length).toBeGreaterThan(0);
    expect(screen.getByText("7")).toBeInTheDocument();
  });

  it("calls repair_codex_state with dryRun true when Dry Run is clicked", async () => {
    render(<CodexStateRepairPanel />);

    fireEvent.click(await screen.findByRole("button", { name: "Dry Run" }));

    await waitFor(() =>
      expect(repairPayloads).toContainEqual({ dryRun: true }),
    );
    expect(
      await screen.findByText("Last result: dry run to", { exact: false }),
    ).toBeInTheDocument();
  });

  it("calls repair_codex_state with dryRun false when Repair is clicked", async () => {
    render(<CodexStateRepairPanel />);

    fireEvent.click(await screen.findByRole("button", { name: /Repair$/ }));

    await waitFor(() =>
      expect(repairPayloads).toContainEqual({ dryRun: false }),
    );
    expect(
      await screen.findByText("Last result: repair to", { exact: false }),
    ).toBeInTheDocument();
  });
});
