import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PageFrame } from "@/components/shell/PageFrame";

function Child() {
  const [count, setCount] = useState(0);
  return <button onClick={() => setCount(count + 1)}>{count}</button>;
}
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it("keeps live child state and cancels interrupted transitions", () => {
  const cancel = vi.fn();
  const animate = vi.fn(() => ({ cancel }));
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
  vi.stubGlobal("Animation", undefined);
  Object.defineProperty(HTMLElement.prototype, "animate", {
    configurable: true,
    value: animate,
  });
  const { rerender, unmount } = render(
    <PageFrame transitionKey="one">
      <Child />
    </PageFrame>,
  );
  fireEvent.click(screen.getByRole("button"));
  rerender(
    <PageFrame transitionKey="two">
      <Child />
    </PageFrame>,
  );
  expect(screen.getByRole("button")).toHaveTextContent("1");
  expect(animate).toHaveBeenCalledTimes(2);
  expect(cancel).toHaveBeenCalledTimes(1);
  unmount();
  expect(cancel).toHaveBeenCalledTimes(2);
  delete (HTMLElement.prototype as { animate?: unknown }).animate;
});
it("skips motion when the system requests reduced motion", () => {
  const animate = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "animate", {
    configurable: true,
    value: animate,
  });
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: true })),
  );
  render(
    <PageFrame transitionKey="one">
      <Child />
    </PageFrame>,
  );
  expect(animate).not.toHaveBeenCalled();
  delete (HTMLElement.prototype as { animate?: unknown }).animate;
});
