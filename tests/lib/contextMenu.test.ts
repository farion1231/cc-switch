import { describe, it, expect } from "vitest";
import { initializeContextMenuGuard } from "@/lib/contextMenu";

function dispatchContextMenu(el: Element): boolean {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
  });
  el.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("contextMenu guard", () => {
  initializeContextMenuGuard();

  it("suppresses the menu on plain app chrome", () => {
    document.body.innerHTML = `<div class="card"><span>label</span></div>`;
    const target = document.querySelector("span")!;
    expect(dispatchContextMenu(target)).toBe(true);
  });

  it("keeps the menu on editable fields", () => {
    document.body.innerHTML = `<div><input /><textarea></textarea>
      <div contenteditable="true"></div></div>`;
    expect(dispatchContextMenu(document.querySelector("input")!)).toBe(false);
    expect(dispatchContextMenu(document.querySelector("textarea")!)).toBe(
      false,
    );
    expect(
      dispatchContextMenu(document.querySelector("[contenteditable]")!),
    ).toBe(false);
  });

  it("keeps the menu on selectable content areas", () => {
    document.body.innerHTML = `<div class="selectable"><p>message</p></div>
      <pre>log line</pre>
      <p><code>inline_code</code></p>`;
    expect(dispatchContextMenu(document.querySelector("p")!)).toBe(false);
    expect(dispatchContextMenu(document.querySelector("pre")!)).toBe(false);
    expect(dispatchContextMenu(document.querySelector("code")!)).toBe(false);
  });
});
