import { act, fireEvent, render } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";

import JsonEditor from "@/components/JsonEditor";

describe("JsonEditor", () => {
  it("updates height and callbacks without recreating the editor view", () => {
    const firstOnChange = vi.fn();
    const secondOnChange = vi.fn();
    const { container, rerender } = render(
      <JsonEditor
        id="configuration-json"
        ariaLabel="Configuration JSON"
        value="{}"
        onChange={firstOnChange}
        height={60}
      />,
    );

    const content = container.querySelector(".cm-content");
    expect(content).not.toBeNull();
    const originalView = EditorView.findFromDOM(content as HTMLElement);
    expect(originalView).toBeDefined();
    expect(content).toHaveAttribute("aria-label", "Configuration JSON");

    rerender(
      <JsonEditor
        id="configuration-json"
        ariaLabel="Configuration JSON"
        value="{}"
        onChange={secondOnChange}
        height={120}
      />,
    );

    const currentContent = container.querySelector(".cm-content");
    const currentView = EditorView.findFromDOM(currentContent as HTMLElement);
    expect(currentView).toBe(originalView);
    expect(container.querySelector("#configuration-json")).toHaveStyle({
      height: "120px",
    });

    act(() => {
      currentView?.dispatch({
        changes: {
          from: 0,
          to: currentView.state.doc.length,
          insert: '{"changed":true}',
        },
      });
    });

    expect(firstOnChange).not.toHaveBeenCalled();
    expect(secondOnChange).toHaveBeenLastCalledWith('{"changed":true}');
  });

  it("keeps the cursor near the edited region when external normalization adds text", () => {
    const original = '{\n  "a": 1,\n  "b": 2\n}';
    const normalized = '{\n  "a": 1,\n  "added": true,\n  "b": 2\n}';
    const { container, rerender } = render(
      <JsonEditor value={original} onChange={vi.fn()} />,
    );
    const content = container.querySelector(".cm-content");
    const view = EditorView.findFromDOM(content as HTMLElement);
    const originalCursor = original.indexOf('"b"') + 1;

    act(() => {
      view?.dispatch({ selection: { anchor: originalCursor } });
    });
    rerender(<JsonEditor value={normalized} onChange={vi.fn()} />);

    expect(view?.state.selection.main.head).toBe(normalized.indexOf('"b"') + 1);
  });

  it("keeps the cursor on unchanged context between separate external edits", () => {
    const original = '{"a":1,"b":2,"c":3}';
    const normalized = '{"a":100,"b":2,"c":300}';
    const { container, rerender } = render(
      <JsonEditor value={original} onChange={vi.fn()} />,
    );
    const content = container.querySelector(".cm-content");
    const view = EditorView.findFromDOM(content as HTMLElement);
    const originalCursor = original.indexOf('"b"') + 1;

    act(() => {
      view?.dispatch({ selection: { anchor: originalCursor } });
    });
    rerender(<JsonEditor value={normalized} onChange={vi.fn()} />);

    expect(view?.state.selection.main.head).toBe(normalized.indexOf('"b"') + 1);
  });
  it("opens search by keyboard and replaces a match without submitting the form", () => {
    const onChange = vi.fn();
    const onSubmit = vi.fn((event) => event.preventDefault());
    const { container } = render(
      <form onSubmit={onSubmit}>
        <JsonEditor
          value="alpha beta alpha"
          onChange={onChange}
          language="javascript"
        />
      </form>,
    );
    const content = container.querySelector(".cm-content") as HTMLElement;
    fireEvent.keyDown(content, {
      key: "f",
      code: "KeyF",
      keyCode: 70,
      ctrlKey: true,
    });
    const panel = container.querySelector(".cm-search")!;
    expect(panel).not.toBeNull();
    const search = panel.querySelector('input[name="search"]')!;
    const replacement = panel.querySelector('input[name="replace"]')!;
    fireEvent.change(search, { target: { value: "alpha" } });
    const view = EditorView.findFromDOM(content)!;
    fireEvent.click(panel.querySelector('button[name="next"]')!);
    expect(view.state.selection.main.from).toBe(0);
    expect(view.state.selection.main.to).toBe(5);
    fireEvent.click(panel.querySelector('button[name="next"]')!);
    expect(view.state.selection.main.from).toBe(11);
    expect(view.state.selection.main.to).toBe(16);
    fireEvent.click(panel.querySelector('button[name="prev"]')!);
    expect(view.state.selection.main.from).toBe(0);
    expect(view.state.selection.main.to).toBe(5);
    fireEvent.change(replacement, { target: { value: "gamma" } });
    fireEvent.click(panel.querySelector('button[name="replaceAll"]')!);
    expect(onChange).toHaveBeenLastCalledWith("gamma beta gamma");
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.click(panel.querySelector('button[name="close"]')!);
    expect(container.querySelector(".cm-search")).toBeNull();
  });

  it("keeps replacement controls out of read-only search panels", () => {
    const { container } = render(
      <JsonEditor value="alpha" onChange={vi.fn()} readOnly />,
    );
    fireEvent.keyDown(container.querySelector(".cm-content")!, {
      key: "f",
      code: "KeyF",
      keyCode: 70,
      ctrlKey: true,
    });
    const panel = container.querySelector(".cm-search")!;
    expect(panel).not.toBeNull();
    expect(panel.querySelector('input[name="search"]')).not.toBeNull();
    expect(panel.querySelector('input[name="replace"]')).toBeNull();
    expect(panel.querySelector('button[name="replaceAll"]')).toBeNull();
  });
});
