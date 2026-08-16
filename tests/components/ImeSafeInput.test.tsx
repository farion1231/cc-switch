import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";

describe("ImeSafeInput", () => {
  it("keeps composition changes local until composition ends", () => {
    const onValueChange = vi.fn();
    const { rerender } = render(
      <ImeSafeInput value="" onValueChange={onValueChange} />,
    );
    const input = screen.getByRole("textbox");

    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "mimo" } });

    expect(input).toHaveValue("mimo");
    expect(onValueChange).not.toHaveBeenCalled();

    // A parent render with the stale committed value must not replace marked
    // text while the platform IME still owns the composition range.
    rerender(<ImeSafeInput value="" onValueChange={onValueChange} />);
    expect(input).toHaveValue("mimo");

    fireEvent.compositionEnd(input, { data: "mimo" });

    expect(onValueChange).toHaveBeenCalledTimes(1);
    expect(onValueChange).toHaveBeenCalledWith("mimo");

    // WebKit can emit a matching input event immediately after compositionend.
    fireEvent.change(input, { target: { value: "mimo" } });
    expect(onValueChange).toHaveBeenCalledTimes(1);
  });

  it("normalizes only the committed composition value", () => {
    const onValueChange = vi.fn();
    render(
      <ImeSafeInput
        value=""
        onValueChange={onValueChange}
        normalize={(value) => value.toLowerCase().replace(/[^a-z0-9-]/g, "")}
      />,
    );
    const input = screen.getByRole("textbox");

    fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "Mi好-1" } });

    expect(input).toHaveValue("Mi好-1");
    expect(onValueChange).not.toHaveBeenCalled();

    fireEvent.compositionEnd(input, { data: "Mi好-1" });

    expect(input).toHaveValue("mi-1");
    expect(onValueChange).toHaveBeenCalledWith("mi-1");
  });

  it("commits and normalizes ordinary input immediately", () => {
    const onValueChange = vi.fn();
    render(
      <ImeSafeInput
        value=""
        onValueChange={onValueChange}
        normalize={(value) => value.toLowerCase().replace(/\s/g, "")}
      />,
    );
    const input = screen.getByRole("textbox");

    fireEvent.change(input, { target: { value: "A B" } });

    expect(input).toHaveValue("ab");
    expect(onValueChange).toHaveBeenCalledWith("ab");
  });

  it("syncs external values while composition is idle", () => {
    const onValueChange = vi.fn();
    const { rerender } = render(
      <ImeSafeInput value="first" onValueChange={onValueChange} />,
    );

    rerender(<ImeSafeInput value="second" onValueChange={onValueChange} />);

    expect(screen.getByRole("textbox")).toHaveValue("second");
  });
});
