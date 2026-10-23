import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { RequestLogColumnsMenu } from "@/components/usage/RequestLogColumnsMenu";

// 「显示的列」在请求日志的表格工具栏里
describe("RequestLogColumnsMenu", () => {
  it("shows and hides request log columns", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn().mockResolvedValue(true);
    render(
      <RequestLogColumnsMenu
        value={{ currency: "CNY", requestLogColumns: { app: false } }}
        onChange={onChange}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "usage.display.logColumns" }),
    );
    expect(
      screen.queryByRole("menuitemcheckbox", { name: "usage.time" }),
    ).not.toBeInTheDocument();
    const status = await screen.findByRole("menuitemcheckbox", {
      name: "usage.statusCode",
    });
    expect(status).toHaveAttribute("aria-checked", "false");
    await user.click(status);

    // 菜单不关，可以接着勾；保存时其它显示设置原样带上，只存改过的列
    expect(onChange).toHaveBeenLastCalledWith({
      currency: "CNY",
      requestLogColumns: { app: false, statusCode: true },
    });
    expect(
      screen.getByRole("menuitemcheckbox", { name: "usage.statusCode" }),
    ).toHaveAttribute("aria-checked", "true");

    // 勾回默认值的列不再写进设置
    await user.click(
      screen.getByRole("menuitemcheckbox", { name: "usage.app" }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      currency: "CNY",
      requestLogColumns: { statusCode: true },
    });

    await user.click(
      screen.getByRole("menuitem", { name: "usage.logColumns.reset" }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      currency: "CNY",
      requestLogColumns: {},
    });
  });

  it("keeps later toggles while an earlier save is still in flight", async () => {
    const user = userEvent.setup();
    const saves: ((ok: boolean) => void)[] = [];
    const onChange = vi.fn(
      () => new Promise<boolean>((resolve) => saves.push(resolve)),
    );
    const { rerender } = render(
      <RequestLogColumnsMenu value={{ currency: "CNY" }} onChange={onChange} />,
    );
    await user.click(
      screen.getByRole("button", { name: "usage.display.logColumns" }),
    );
    const item = (name: string) =>
      screen.getByRole("menuitemcheckbox", { name });
    await user.click(
      await screen.findByRole("menuitemcheckbox", {
        name: "usage.logColumns.duration",
      }),
    );
    await user.click(item("usage.statusCode"));

    // 第一次保存先回来、设置读回只含第一列：后勾的列不能被冲掉
    rerender(
      <RequestLogColumnsMenu
        value={{ currency: "CNY", requestLogColumns: { duration: true } }}
        onChange={onChange}
      />,
    );
    await act(async () => saves[0](true));
    expect(item("usage.statusCode")).toHaveAttribute("aria-checked", "true");

    // 最后一次保存失败：退回到设置里实际存的
    await act(async () => saves[1](false));
    expect(item("usage.statusCode")).toHaveAttribute("aria-checked", "false");
    expect(item("usage.logColumns.duration")).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });
});
