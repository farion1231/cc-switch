import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDateRangePicker } from "@/components/usage/UsageDateRangePicker";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?:
        | string
        | {
            defaultValue?: string;
          },
    ) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/components/ui/popover", () => ({
  Popover: ({ children }: any) => <div>{children}</div>,
  PopoverTrigger: ({ children }: any) => <>{children}</>,
  PopoverContent: ({ children }: any) => <div>{children}</div>,
}));

function localTs(
  year: number,
  monthIndex: number,
  day: number,
  hour: number,
  minute: number,
  second: number,
) {
  return Math.floor(
    new Date(year, monthIndex, day, hour, minute, second).getTime() / 1000,
  );
}

describe("UsageDateRangePicker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 5, 22, 11, 23, 45));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses the full local day when a single calendar date is selected", () => {
    const onApply = vi.fn();

    render(
      <UsageDateRangePicker
        selection={{ preset: "1d" }}
        triggerLabel="1d"
        onApply={onApply}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "6/1/2026" }));
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));

    expect(onApply).toHaveBeenCalledWith({
      preset: "custom",
      customStartDate: localTs(2026, 5, 1, 0, 0, 0),
      customEndDate: localTs(2026, 5, 1, 23, 59, 59),
      liveEndTime: false,
    });
  });

  it("defaults an edited end date to the end of that local day", () => {
    const onApply = vi.fn();
    const start = localTs(2026, 5, 1, 0, 0, 0);

    const { container } = render(
      <UsageDateRangePicker
        selection={{
          preset: "custom",
          customStartDate: start,
          customEndDate: localTs(2026, 5, 1, 23, 59, 59),
        }}
        triggerLabel="Custom"
        onApply={onApply}
      />,
    );

    const dateInputs =
      container.querySelectorAll<HTMLInputElement>('input[type="date"]');
    fireEvent.change(dateInputs[1], { target: { value: "2026-06-03" } });
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));

    expect(onApply).toHaveBeenCalledWith({
      preset: "custom",
      customStartDate: start,
      customEndDate: localTs(2026, 5, 3, 23, 59, 59),
      liveEndTime: false,
    });
  });

  it("preserves a drafted end date when refining the start date from a preset", () => {
    const onApply = vi.fn();

    const { container } = render(
      <UsageDateRangePicker
        selection={{ preset: "1d" }}
        triggerLabel="1d"
        onApply={onApply}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "6/1/2026" }));
    fireEvent.click(screen.getByRole("button", { name: "6/3/2026" }));

    const dateInputs =
      container.querySelectorAll<HTMLInputElement>('input[type="date"]');
    fireEvent.focus(dateInputs[0]);
    fireEvent.click(screen.getByRole("button", { name: "6/2/2026" }));
    fireEvent.click(screen.getByRole("button", { name: "common.confirm" }));

    expect(onApply).toHaveBeenCalledWith({
      preset: "custom",
      customStartDate: localTs(2026, 5, 2, 0, 0, 0),
      customEndDate: localTs(2026, 5, 3, 23, 59, 59),
      liveEndTime: false,
    });
  });
});
