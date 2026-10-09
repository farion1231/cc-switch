import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SkillInstallProgress } from "@/components/skills/SkillInstallProgress";

describe("SkillInstallProgress", () => {
  it("shows the download percentage and size from actual bytes", () => {
    render(
      <SkillInstallProgress
        name="example"
        progress={{
          phase: "downloading",
          downloadedBytes: 1048576,
          totalBytes: 4194304,
        }}
      />,
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "25",
    );
    expect(screen.getByText("1.0 MiB / 4.0 MiB")).toBeInTheDocument();
  });

  it("shows received bytes without inventing a percentage when the total is unknown", () => {
    render(
      <SkillInstallProgress
        name="example"
        progress={{
          phase: "downloading",
          downloadedBytes: 2048,
          totalBytes: null,
        }}
      />,
    );
    expect(screen.getByRole("progressbar")).not.toHaveAttribute(
      "aria-valuenow",
    );
    expect(screen.getByText("2.0 KiB")).toBeInTheDocument();
    expect(screen.queryByText(/%/)).not.toBeInTheDocument();
  });

  it("switches to indeterminate extraction after download and completes only after installation", () => {
    const view = render(
      <SkillInstallProgress
        name="example"
        progress={{
          phase: "downloading",
          downloadedBytes: 4096,
          totalBytes: 4096,
        }}
      />,
    );
    expect(screen.getByText("100%")).toBeInTheDocument();
    view.rerender(
      <SkillInstallProgress
        name="example"
        progress={{
          phase: "extracting",
          downloadedBytes: 4096,
          totalBytes: 4096,
        }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "skills.installProgress.extracting",
    );
    expect(screen.getByRole("progressbar")).not.toHaveAttribute(
      "aria-valuenow",
    );
    view.rerender(
      <SkillInstallProgress
        name="example"
        progress={{ phase: "completed", downloadedBytes: 0, totalBytes: null }}
      />,
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "100",
    );
  });
});
