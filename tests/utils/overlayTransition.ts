import { act } from "@testing-library/react";

export async function waitForOverlayTransition() {
  await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
}
