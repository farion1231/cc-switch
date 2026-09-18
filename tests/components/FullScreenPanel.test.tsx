import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  FullScreenPanel,
  subscribeToFullScreenPanelDismiss,
} from "@/components/common/FullScreenPanel";

const Panels = ({ innerOpen }: { innerOpen: boolean }) => (
  <>
    <FullScreenPanel isOpen title="Outer" onClose={() => undefined}>
      outer
    </FullScreenPanel>
    <FullScreenPanel isOpen={innerOpen} title="Inner" onClose={() => undefined}>
      inner
    </FullScreenPanel>
  </>
);

describe("FullScreenPanel body scroll locking", () => {
  afterEach(() => {
    document.body.style.overflow = "";
  });

  it("keeps the body locked when a nested panel closes", () => {
    document.body.style.overflow = "clip";
    const view = render(<Panels innerOpen />);

    expect(document.body.style.overflow).toBe("hidden");

    view.rerender(<Panels innerOpen={false} />);
    expect(document.body.style.overflow).toBe("hidden");

    view.unmount();
    expect(document.body.style.overflow).toBe("clip");
  });
});

/** The full-screen surface is the `position: fixed` ancestor of the panel title. */
const surfaceOf = (title: string): HTMLElement | null => {
  let node: HTMLElement | null = screen.getByText(title);
  while (node && !node.className.includes("fixed")) {
    node = node.parentElement;
  }
  return node;
};

describe("FullScreenPanel entry motion", () => {
  it("renders its opaque surface on the first frame instead of fading it in from transparent", () => {
    render(
      <FullScreenPanel isOpen title="Panel" onClose={() => undefined}>
        body
      </FullScreenPanel>,
    );

    const surface = surfaceOf("Panel");
    expect(surface).not.toBeNull();

    // The surface is a fully opaque, full-screen layer. Mounting it at
    // `opacity: 0` cross-fades it with the page underneath, so for the whole
    // transition the user sees two screens blended together — which reads as a
    // flicker on both open and close. The surface itself must be opaque from
    // the first frame; any entry animation belongs on the panel's content.
    expect(surface!.style.opacity).not.toBe("0");
  });
});

describe("FullScreenPanel dismiss notification", () => {
  it("notifies only when the last panel closes, not for a nested one", () => {
    const onDismiss = vi.fn();
    const unsubscribe = subscribeToFullScreenPanelDismiss(onDismiss);
    try {
      const view = render(<Panels innerOpen />);
      expect(onDismiss).not.toHaveBeenCalled();

      // Inner closes, outer still open: the page is NOT back yet.
      view.rerender(<Panels innerOpen={false} />);
      expect(onDismiss).not.toHaveBeenCalled();

      // Last panel closes: now the page is revealed.
      view.unmount();
      expect(onDismiss).toHaveBeenCalledTimes(1);
    } finally {
      unsubscribe();
    }
  });
});
