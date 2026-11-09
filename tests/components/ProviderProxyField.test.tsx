import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { Form } from "@/components/ui/form";
import { ProviderProxyField } from "@/components/providers/forms/ProviderProxyField";

const globalProxy = vi.hoisted(() => ({ url: null as string | null }));

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({ data: globalProxy.url }),
  useTestProxy: () => ({ mutate: vi.fn(), isPending: false }),
}));

function Harness({
  initial,
  onChange = vi.fn(),
}: {
  initial: string;
  onChange?: (value: string) => void;
}) {
  const form = useForm();
  const [value, setValue] = useState(initial);
  return (
    <Form {...form}>
      <ProviderProxyField
        id="proxy"
        value={value}
        onChange={(next) => {
          setValue(next);
          onChange(next);
        }}
      />
    </Form>
  );
}

const urlInput = () =>
  screen.getByPlaceholderText(
    "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
  );
const usernameInput = () =>
  screen.getByLabelText(/settings\.globalProxy\.username|用户名/);
const passwordInput = () =>
  screen.getByLabelText(/settings\.globalProxy\.password|密码/);
const globalNotice = /providerForm\.proxyUrlGlobalEnabled|已启用全局代理/;

describe("ProviderProxyField", () => {
  beforeEach(() => {
    globalProxy.url = null;
  });

  it("splits a stored URL into address, username and password", () => {
    render(<Harness initial="http://user:secret@127.0.0.1:7890" />);
    expect(urlInput()).toBeEnabled();
    expect(urlInput()).toHaveValue("http://127.0.0.1:7890");
    expect(usernameInput()).toHaveValue("user");
    expect(passwordInput()).toHaveValue("secret");
    expect(screen.queryByText(globalNotice)).toBeNull();
  });

  it("merges username and password into the saved URL", () => {
    const onChange = vi.fn();
    render(<Harness initial="" onChange={onChange} />);
    fireEvent.change(urlInput(), {
      target: { value: "socks5://127.0.0.1:1080" },
    });
    fireEvent.change(usernameInput(), { target: { value: "user" } });
    fireEvent.change(passwordInput(), { target: { value: "p@ss" } });
    expect(onChange).toHaveBeenLastCalledWith(
      "socks5://user:p%40ss@127.0.0.1:1080",
    );
  });

  it("is disabled and shows the global proxy while it is on", () => {
    globalProxy.url = "http://gu:gp@127.0.0.1:7890";
    render(<Harness initial="socks5://10.0.0.2:1080" />);
    expect(urlInput()).toBeDisabled();
    expect(urlInput()).toHaveValue("http://127.0.0.1:7890");
    expect(usernameInput()).toBeDisabled();
    expect(usernameInput()).toHaveValue("gu");
    expect(passwordInput()).toBeDisabled();
    expect(passwordInput()).toHaveAttribute("type", "password");
    expect(screen.getByText(globalNotice)).toBeInTheDocument();
  });

  it("flags an unsupported scheme", () => {
    render(<Harness initial="ftp://127.0.0.1:21" />);
    expect(urlInput()).toHaveAttribute("aria-invalid", "true");
  });
});
