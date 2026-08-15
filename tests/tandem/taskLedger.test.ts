import { describe, expect, it } from "vitest";
import { buildTaskLedger } from "@/tandem/taskLedger";
import type { Project, TaskLedgerItem, TaskStatus } from "@/tandem/types";

const project: Project = {
  id: "project-1",
  name: "CC Switch",
  rootPath: "/projects/cc-switch",
  createdAt: 1,
  updatedAt: 1,
};

const item = (
  id: string,
  status: TaskStatus,
  updatedAt: number,
): TaskLedgerItem => ({
  project: { ...project },
  task: {
    id,
    projectId: project.id,
    title: `Task ${id}`,
    originalInstruction: `Instruction ${id}`,
    status,
    createdAt: updatedAt - 1,
    updatedAt,
    completedAt: status === "completed" ? updatedAt : null,
  },
});

describe("buildTaskLedger", () => {
  it("classifies statuses into the exact visible sections and omits completed tasks", () => {
    const ledger = buildTaskLedger([
      item("attention", "needs_attention", 10),
      item("acceptance", "awaiting_acceptance", 20),
      item("active", "active", 30),
      item("paused", "paused", 40),
      item("completed", "completed", 50),
    ]);

    expect(ledger.needsAttention.map(({ task }) => task.id)).toEqual([
      "attention",
    ]);
    expect(ledger.awaitingAcceptance.map(({ task }) => task.id)).toEqual([
      "acceptance",
    ]);
    expect(ledger.active.map(({ task }) => task.id)).toEqual(["active"]);
    expect(ledger.recentResumable.map(({ task }) => task.id)).toEqual([
      "paused",
    ]);
    expect(
      [
        ...ledger.needsAttention,
        ...ledger.awaitingAcceptance,
        ...ledger.active,
        ...ledger.recentResumable,
      ].map(({ task }) => task.id),
    ).not.toContain("completed");
  });

  it("orders every section newest first with task ID as ascending tie-breaker", () => {
    const statuses: TaskStatus[] = [
      "needs_attention",
      "awaiting_acceptance",
      "active",
      "paused",
    ];

    for (const status of statuses) {
      const ledger = buildTaskLedger([
        item(`${status}-z`, status, 100),
        item(`${status}-old`, status, 90),
        item(`${status}-a`, status, 100),
      ]);
      const section =
        status === "needs_attention"
          ? ledger.needsAttention
          : status === "awaiting_acceptance"
            ? ledger.awaitingAcceptance
            : status === "active"
              ? ledger.active
              : ledger.recentResumable;

      expect(section.map(({ task }) => task.id)).toEqual([
        `${status}-a`,
        `${status}-z`,
        `${status}-old`,
      ]);
    }
  });

  it("uses locale-independent UTF-16 ordinal order for ID ties", () => {
    const ledger = buildTaskLedger([
      item("a", "active", 100),
      item("Z", "active", 100),
    ]);

    expect(ledger.active.map(({ task }) => task.id)).toEqual(["Z", "a"]);
  });

  it("limits recent paused tasks to ten", () => {
    const paused = Array.from({ length: 12 }, (_, index) =>
      item(`paused-${index.toString().padStart(2, "0")}`, "paused", index),
    );

    expect(
      buildTaskLedger(paused).recentResumable.map(({ task }) => task.id),
    ).toEqual([
      "paused-11",
      "paused-10",
      "paused-09",
      "paused-08",
      "paused-07",
      "paused-06",
      "paused-05",
      "paused-04",
      "paused-03",
      "paused-02",
    ]);
  });

  it("does not mutate its input or nested values", () => {
    const input = [
      item("active-b", "active", 2),
      item("active-a", "active", 3),
    ];
    const snapshot = structuredClone(input);

    buildTaskLedger(input);

    expect(input).toEqual(snapshot);
  });
});
