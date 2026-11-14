import { fireEvent, screen } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { useForm } from "react-hook-form";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";
import { ProxyRateLimitField } from "@/components/providers/forms/ProxyRateLimitField";
import { Form, FormField } from "@/components/ui/form";

// FormLabel 需要 FormField 上下文，和真实表单里的挂载方式保持一致。
const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return (
    <Form {...form}>
      <FormField name="proxyRateLimit" render={() => <>{children}</>} />
    </Form>
  );
};

describe("ProxyRateLimitField", () => {
  it("stays off by default and only records the switch when enabling", () => {
    const onChange = vi.fn();
    render(
      <FormShell>
        <ProxyRateLimitField value={undefined} onChange={onChange} />
      </FormShell>,
    );

    const toggle = screen.getByRole("switch");
    expect(toggle).not.toBeChecked();
    // 测试环境没有翻译资源，t() 回落到 defaultValue。
    expect(screen.getByText(/默认关闭/)).toBeInTheDocument();
    expect(screen.queryByPlaceholderText("18")).not.toBeInTheDocument();

    fireEvent.click(toggle);

    // 不开启时不预设数值，避免把默认值写进用户配置。
    expect(onChange).toHaveBeenCalledWith({ enabled: true });
  });

  it("shows defaults as placeholders and reports edited numbers", () => {
    const onChange = vi.fn();
    render(
      <FormShell>
        <ProxyRateLimitField value={{ enabled: true }} onChange={onChange} />
      </FormShell>,
    );

    const rpm = screen.getByPlaceholderText("18");
    fireEvent.change(rpm, { target: { value: "12" } });

    expect(onChange).toHaveBeenCalledWith({ enabled: true, rpm: 12 });
  });

  it("keeps existing values when disabling", () => {
    const onChange = vi.fn();
    render(
      <FormShell>
        <ProxyRateLimitField
          value={{ enabled: true, rpm: 20, burst: 3, maxWaitMs: 15000 }}
          onChange={onChange}
        />
      </FormShell>,
    );

    fireEvent.click(screen.getByRole("switch"));

    expect(onChange).toHaveBeenCalledWith({
      enabled: false,
      rpm: 20,
      burst: 3,
      maxWaitMs: 15000,
    });
  });
});
