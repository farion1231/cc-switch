import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  MIN_REFRESH_SPIN_MS,
  QuotaBars,
  QuotaLines,
  REFRESH_RESULT_MS,
} from "@/components/quota/QuotaLines";
import { failedLines, type QuotaLine } from "@/components/quota/quotaRules";
import type { TFunction } from "i18next";

const t = ((key: string) => key) as unknown as TFunction;
const lines: QuotaLine[] = [
  { key: "five_hour", text: "5 小时剩余 82%", tone: "normal", left: 82 },
];

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

const icon = () => screen.queryByTestId("quota-refresh-icon");
const isSpinning = () =>
  icon()?.classList.contains("motion-safe:animate-spin") ?? false;
const result = () =>
  screen.queryByTestId("quota-refresh-succeeded")
    ? "✓"
    : screen.queryByTestId("quota-refresh-failed")
      ? "✗"
      : null;

/** 让 refetch() 的 Promise 落定 */
const flush = () => act(async () => await Promise.resolve());

describe("quota column refresh", () => {
  it("keeps the refresh icon out of sight until hover or focus", () => {
    render(<QuotaLines lines={lines} onRefresh={vi.fn()} />);
    expect(icon()).toHaveClass("hidden", "group-hover:block");
    expect(isSpinning()).toBe(false);
  });

  it("spins from the click until the query is done, then ticks for a moment", () => {
    const onRefresh = vi.fn();
    const { rerender } = render(
      <QuotaLines lines={lines} onRefresh={onRefresh} />,
    );

    fireEvent.click(screen.getByRole("button"));
    expect(onRefresh).toHaveBeenCalledTimes(1);
    expect(isSpinning()).toBe(true);

    // 查询还在跑：过了最短时长也继续转，且不能再点
    rerender(<QuotaLines lines={lines} onRefresh={onRefresh} loading />);
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS * 2));
    expect(isSpinning()).toBe(true);
    expect(screen.getByRole("button")).toHaveAttribute("aria-disabled", "true");
    // 转圈时再点不会重复查
    fireEvent.click(screen.getByRole("button"));
    expect(onRefresh).toHaveBeenCalledTimes(1);

    // 查完 → ✓，停一会儿再消失
    rerender(<QuotaLines lines={lines} onRefresh={onRefresh} />);
    act(() => vi.advanceTimersByTime(0));
    expect(result()).toBe("✓");
    act(() => vi.advanceTimersByTime(REFRESH_RESULT_MS));
    expect(result()).toBeNull();
    expect(isSpinning()).toBe(false);
  });

  it("stays quiet after the tick until the pointer leaves", () => {
    render(<QuotaLines lines={lines} onRefresh={vi.fn()} />);
    const button = screen.getByRole("button");
    fireEvent.click(button);
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS));
    act(() => vi.advanceTimersByTime(REFRESH_RESULT_MS));

    // ✓ 消失后鼠标还停在上面：↻ 不跟着冒出来
    expect(icon()).toHaveClass("hidden");
    expect(icon()).not.toHaveClass("group-hover:block");

    // 移出再移进来才重新可见
    fireEvent.mouseLeave(button);
    expect(icon()).toHaveClass("group-hover:block");
  });

  it("keeps spinning for the minimum time when the query returns at once", () => {
    render(<QuotaLines lines={lines} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByRole("button"));

    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS - 1));
    expect(isSpinning()).toBe(true);
    act(() => vi.advanceTimersByTime(1));
    expect(result()).toBe("✓");
  });

  it("crosses when the column turns into a failure", () => {
    const { rerender } = render(
      <QuotaLines lines={lines} onRefresh={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button"));
    rerender(
      <QuotaLines lines={failedLines(t, "HTTP 500")} onRefresh={vi.fn()} />,
    );
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS));
    expect(result()).toBe("✗");
  });

  it("crosses a failed retry even when the column already showed the failure", () => {
    const failed = failedLines(t, "HTTP 500");
    render(<QuotaLines lines={failed} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByRole("button"));
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS));
    expect(result()).toBe("✗");
  });

  it("crosses when the request failed but the last good quota is still shown", async () => {
    render(
      <QuotaLines
        lines={lines}
        onRefresh={() => Promise.resolve({ isError: true })}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    await flush();
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS));
    expect(result()).toBe("✗");
  });

  it("waits for the refetch promise before deciding", async () => {
    let resolve!: (value: unknown) => void;
    render(
      <QuotaLines
        lines={lines}
        onRefresh={() => new Promise((r) => (resolve = r))}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    act(() => vi.advanceTimersByTime(MIN_REFRESH_SPIN_MS * 3));
    expect(isSpinning()).toBe(true);

    resolve({ isError: false });
    await flush();
    act(() => vi.advanceTimersByTime(0));
    expect(result()).toBe("✓");
  });

  it("does not spin for background refreshes, only dims", () => {
    render(<QuotaLines lines={lines} onRefresh={vi.fn()} loading />);
    expect(isSpinning()).toBe(false);
    expect(screen.getByRole("button")).toHaveClass("opacity-60");
  });

  it("shows no icon when the quota cannot be refreshed", () => {
    render(<QuotaLines lines={lines} />);
    expect(icon()).toBeNull();
    expect(result()).toBeNull();
  });

  it("splits off the resets as a dropdown, the rest of the column still refreshes", () => {
    const onRefresh = vi.fn();
    const withResets: QuotaLine[] = [
      { ...lines[0], short: "5 小时 82%", window: 0 },
      {
        key: "seven_day",
        text: "每周剩余 64%",
        short: "每周 64%",
        tone: "normal",
        left: 64,
        window: 2,
      },
      {
        key: "reset_credits",
        text: "重置剩余 3 次",
        short: "重置 3 次",
        tone: "normal",
        left: Infinity,
        window: 99,
        breakdown: {
          title: "存下的限额重置",
          openLabel: "查看到期时间",
          items: [],
        },
      },
    ];
    render(<QuotaLines lines={withResets} onRefresh={onRefresh} />);

    // 只有一个刷新按钮（键盘、读屏认它），重置次数自己是按钮
    expect(screen.getAllByRole("button")).toHaveLength(2);
    expect(icon()).toHaveClass("hidden");

    // 同一行的「每周」也能悬停露 ↻、点了重查
    const weekly = screen.getByText("每周 64%");
    fireEvent.mouseEnter(weekly);
    expect(icon()).not.toHaveClass("hidden");
    fireEvent.click(weekly);
    expect(onRefresh).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "查看到期时间" }));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});

describe("expanded quota bars", () => {
  it("writes the reset countdown after the value", () => {
    vi.setSystemTime(new Date("2026-10-04T10:00:00Z"));
    render(
      <QuotaBars
        rows={[
          {
            label: "5 小时",
            line: {
              key: "five_hour",
              text: "5 小时剩余 69%",
              value: "剩余 69%",
              tone: "normal",
              left: 69,
              label: "5 小时",
              resetsAt: "2026-10-04T12:30:00Z",
            },
          },
        ]}
      />,
    );
    // 测试里 t() 返回 key
    expect(screen.getByText("subscription.resetsIn")).toBeInTheDocument();
  });
});

describe("reset countdown on the card", () => {
  it("only widens the column when some tier has a reset time", () => {
    vi.setSystemTime(new Date("2026-10-04T10:00:00Z"));
    const { container, rerender } = render(<QuotaLines lines={lines} />);
    expect(container.firstElementChild).toHaveClass("w-[136px]");
    expect(container.querySelector("svg")).toBeNull();

    rerender(
      <QuotaLines
        lines={[{ ...lines[0], resetsAt: "2026-10-04T12:30:00Z" }]}
      />,
    );
    expect(container.firstElementChild).toHaveClass("w-[194px]");
    expect(screen.getByText("2h30m")).toBeInTheDocument();
  });
});
