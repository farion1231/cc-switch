import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SessionTocSidebar } from "@/components/sessions/SessionToc";

it("selects complete turns, represents partial turns, and keeps navigation separate", () => {
  const navigate = vi.fn();
  const selections: number[][] = [];
  function Harness() {
    const [selected, setSelected] = useState(new Set([1]));
    return (
      <SessionTocSidebar
        items={[
          { index: 0, preview: "First question" },
          { index: 3, preview: "Last question" },
        ]}
        onItemClick={navigate}
        selection={{
          selected,
          messageCount: 5,
          onChange: (start, end, checked) => {
            const next = new Set(selected);
            for (let i = start; i < end; i++)
              checked ? next.add(i) : next.delete(i);
            selections.push([...next].sort());
            setSelected(next);
          },
        }}
      />
    );
  }
  render(<Harness />);
  const [first, last] = screen.getAllByRole("checkbox");
  expect(first).toBePartiallyChecked();
  fireEvent.click(first);
  expect(first).toBeChecked();
  expect(selections.at(-1)).toEqual([0, 1, 2]);
  fireEvent.click(last);
  expect(selections.at(-1)).toEqual([0, 1, 2, 3, 4]);
  fireEvent.click(first);
  expect(selections.at(-1)).toEqual([3, 4]);
  expect(navigate).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /Last question/ }));
  expect(navigate).toHaveBeenCalledWith(3);
  expect(last).toBeChecked();
});
