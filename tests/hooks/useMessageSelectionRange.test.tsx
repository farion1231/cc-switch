import { createRef } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useMessageSelectionRange } from "@/hooks/useMessageSelectionRange";

afterEach(() => {
  document.getSelection()!.removeAllRanges();
  document.querySelector("#messages")?.remove();
});

it("keeps a reverse selection and the intervening rows mounted, then releases them", () => {
  const container = document.createElement("div");
  container.id = "messages";
  container.innerHTML =
    '<div data-index="2">second</div><div data-index="4">fourth</div>';
  document.body.append(container);
  const ref = createRef<HTMLDivElement>();
  Object.assign(ref, { current: container });
  const { result, rerender } = renderHook(
    ({ session }) => useMessageSelectionRange(ref, session),
    { initialProps: { session: "first" } },
  );
  const range = { startIndex: 20, endIndex: 22, overscan: 1, count: 100 };
  expect(result.current(range)).toEqual([19, 20, 21, 22, 23]);
  act(() => {
    const selection = document.getSelection()!;
    selection.setBaseAndExtent(
      container.lastChild!,
      1,
      container.firstChild!.firstChild!,
      0,
    );
    document.dispatchEvent(new Event("selectionchange"));
  });
  expect(result.current(range)).toEqual(
    Array.from({ length: 22 }, (_, i) => i + 2),
  );
  act(() => {
    document.getSelection()!.removeAllRanges();
    document.dispatchEvent(new Event("selectionchange"));
  });
  expect(result.current(range)).toEqual([19, 20, 21, 22, 23]);
  act(() => {
    document
      .getSelection()!
      .setBaseAndExtent(container.firstChild!, 0, container.lastChild!, 1);
    document.dispatchEvent(new Event("selectionchange"));
  });
  rerender({ session: "second" });
  expect(result.current(range)).toEqual([19, 20, 21, 22, 23]);
});
