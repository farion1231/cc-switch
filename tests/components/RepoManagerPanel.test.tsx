import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";

const m = vi.hoisted(() => ({ getTimeout: vi.fn(), setTimeout: vi.fn() }));
vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    getDownloadTimeout: m.getTimeout,
    setDownloadTimeout: m.setTimeout,
  },
}));

function renderPanel() {
  return render(
    <RepoManagerPanel
      repos={[]}
      skills={[]}
      onAdd={vi.fn()}
      onRemove={vi.fn()}
      onClose={vi.fn()}
    />,
  );
}

describe("Repository download timeout", () => {
  beforeEach(() => {
    m.getTimeout.mockReset().mockResolvedValue(60);
    m.setTimeout.mockReset().mockResolvedValue(undefined);
  });

  it("loads, saves and restores the persisted timeout when reopened", async () => {
    const first = renderPanel();
    const input = screen.getByRole("spinbutton", {
      name: "skills.repo.downloadTimeout",
    });
    await waitFor(() => expect(input).toHaveValue(60));
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    await userEvent.clear(input);
    await userEvent.type(input, "900");
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(m.setTimeout).toHaveBeenCalledWith(900));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "skills.repo.timeoutSaved",
    );
    first.unmount();
    m.getTimeout.mockResolvedValue(900);
    renderPanel();
    await waitFor(() =>
      expect(screen.getByRole("spinbutton")).toHaveValue(900),
    );
  });

  it.each(["", "0", "3601", "1.5"])(
    "rejects invalid value %s without discarding the input",
    async (value) => {
      renderPanel();
      const input = screen.getByRole("spinbutton");
      await waitFor(() => expect(input).toHaveValue(60));
      await userEvent.clear(input);
      if (value) await userEvent.type(input, value);
      await userEvent.click(
        screen.getByRole("button", { name: "common.save" }),
      );
      expect(
        screen.getByText("skills.repo.timeoutInvalid"),
      ).toBeInTheDocument();
      expect(input).toHaveAttribute("aria-invalid", "true");
      expect(m.setTimeout).not.toHaveBeenCalled();
    },
  );

  it("keeps the input and allows retry after a save failure", async () => {
    m.setTimeout.mockRejectedValueOnce(new Error("disk full"));
    renderPanel();
    const input = screen.getByRole("spinbutton");
    await waitFor(() => expect(input).toHaveValue(60));
    await userEvent.clear(input);
    await userEvent.type(input, "300");
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(
      await screen.findByText("skills.repo.timeoutSaveFailed"),
    ).toBeInTheDocument();
    expect(input).toHaveValue(300);
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(
      await screen.findByText("skills.repo.timeoutSaved"),
    ).toBeInTheDocument();
  });

  it("disables editing after a load failure and recovers on retry", async () => {
    m.getTimeout.mockRejectedValueOnce(new Error("unavailable"));
    renderPanel();
    expect(
      await screen.findByText("skills.repo.timeoutLoadFailed"),
    ).toBeInTheDocument();
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(screen.getByRole("spinbutton")).toHaveValue(60));
    expect(screen.getByRole("spinbutton")).toBeEnabled();
  });
});
