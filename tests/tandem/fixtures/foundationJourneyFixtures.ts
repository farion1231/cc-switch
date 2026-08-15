import type { TaskLedgerItem, TaskStatus } from "@/tandem/types";
const row = (
  id: string,
  title: string,
  status: TaskStatus,
  instruction: string,
  updatedAt: number,
): TaskLedgerItem => ({
  project: {
    id: "foundation-project-" + id,
    name: "Tandem Foundation",
    rootPath: "/tmp/tandem-foundation",
    createdAt: updatedAt - 100,
    updatedAt,
  },
  task: {
    id: "foundation-" + id,
    projectId: "foundation-project-" + id,
    title,
    originalInstruction: instruction,
    status,
    createdAt: updatedAt - 100,
    updatedAt,
    completedAt: null,
  },
});
export const foundationJourneyFixtures = (): TaskLedgerItem[] => [
  row(
    "attention",
    "Resolve foundation alert",
    "needs_attention",
    "PRIVATE-LIST-INSTRUCTION",
    1_700_000_000_400,
  ),
  row(
    "acceptance",
    "Review foundation acceptance",
    "awaiting_acceptance",
    "PRIVATE-ACCEPTANCE-INSTRUCTION",
    1_700_000_000_300,
  ),
  row(
    "active",
    "Continue foundation build",
    "active",
    "PRIVATE-LIST-INSTRUCTION-ACTIVE",
    1_700_000_000_200,
  ),
  row(
    "paused",
    "Resume foundation task",
    "paused",
    "PRIVATE-LIST-INSTRUCTION-PAUSED",
    1_700_000_000_100,
  ),
];
