import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";

const platform = vi.hoisted(() => ({ linux: true }));
vi.mock("@/lib/platform", () => ({ isLinux: () => platform.linux }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string | { defaultValue?: string }) =>
      typeof fallback === "string" ? fallback : (fallback?.defaultValue ?? key),
    i18n: { language: "en" },
  }),
}));

const start = new Date(2026, 9, 1, 10, 15);
const end = new Date(2026, 9, 2, 12, 30);
function renderPicker(liveEndTime = false) {
  const onApply = vi.fn();
  render(
    <UsageDateRangePicker
      selection={{
        preset: "custom",
        customStartDate: start.getTime() / 1000,
        customEndDate: end.getTime() / 1000,
        liveEndTime,
      }}
      triggerLabel="range"
      onApply={onApply}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "range" }));
  return onApply;
}

beforeEach(() => {
  platform.linux = true;
  Element.prototype.scrollIntoView = vi.fn();
});

describe("UsageDateRangePicker Linux controls", () => {
  it("uses the inline calendar and closes the time menu after selecting hours and minutes", async () => {
    const user = userEvent.setup();
    const onApply = renderPicker();
    expect(
      document.querySelector('input[type="date"], input[type="time"]'),
    ).toBeNull();
    await user.click(
      screen.getByRole("button", { name: "开始时间: 2026-10-01" }),
    );
    const hour = screen.getByRole("combobox", {
      name: "开始时间 · usage.hour",
    });
    await user.click(hour);
    await user.click(screen.getByRole("option", { name: "07" }));
    await waitFor(() =>
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument(),
    );
    const minute = screen.getByRole("combobox", {
      name: "开始时间 · usage.minute",
    });
    await user.click(minute);
    await user.click(screen.getByRole("option", { name: "23" }));
    await waitFor(() =>
      expect(screen.queryByRole("listbox")).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.confirm" }));
    const expected = new Date(start);
    expected.setHours(7, 23);
    expect(onApply).toHaveBeenCalledWith(
      expect.objectContaining({ customStartDate: expected.getTime() / 1000 }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("disables end time controls in live mode", () => {
    renderPicker(true);
    expect(
      screen.getByRole("combobox", { name: "结束时间 · usage.hour" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("combobox", { name: "结束时间 · usage.minute" }),
    ).toBeDisabled();
  });

  it("preserves native date and time inputs on other platforms", () => {
    platform.linux = false;
    renderPicker();
    expect(document.querySelectorAll('input[type="date"]')).toHaveLength(2);
    expect(document.querySelectorAll('input[type="time"]')).toHaveLength(2);
  });
});
