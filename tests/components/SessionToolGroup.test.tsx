import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { SessionToolGroup } from "@/components/sessions/SessionMessageItem";
import type { SessionToolItem } from "@/components/sessions/utils";

const lines = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`);
const item: SessionToolItem = {
  kind: "tools",
  key: "tools-1",
  messageIndex: 1,
  messageIndexes: [1],
  names: ["Bash"],
  output: [...lines.slice(0, 18), "needle here", "line 20"].join("\n"),
};

describe("SessionToolGroup", () => {
  it("keeps the output collapsed by default", () => {
    render(<SessionToolGroup item={item} isActive={false} />);
    expect(screen.getByRole("button", { expanded: false })).toBeInTheDocument();
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
  });

  it("expands the output, past the preview, when the search hits inside it", () => {
    // 查找命中在第 19 行：折叠着高亮根本看不见，预览只有 12 行也不够
    render(
      <SessionToolGroup item={item} isActive={true} searchQuery="needle" />,
    );
    expect(screen.getByRole("button", { expanded: true })).toBeInTheDocument();
    expect(screen.getByRole("region")).toHaveTextContent("needle here");
    expect(screen.getByRole("region")).toHaveTextContent("line 20");
  });

  it("only expands the preview when the hit is within it", () => {
    render(
      <SessionToolGroup item={item} isActive={false} searchQuery="line 3" />,
    );
    const region = screen.getByRole("region");
    expect(region).toHaveTextContent("line 3");
    expect(region).not.toHaveTextContent("needle here");
  });

  it("shows the tail when the query hits both inside and past the preview", () => {
    // "line 1" 在第 1 行就命中，但第 13–18 行也命中：只问预览里有没有会把后面的藏住，
    // 而查找计数把它们都算了进去，跳到第二个结果却看不见
    render(
      <SessionToolGroup item={item} isActive={false} searchQuery="line 1" />,
    );
    const region = screen.getByRole("region");
    expect(region).toHaveTextContent("line 18");
    expect(region).toHaveTextContent("line 20");
  });

  it("stays collapsed when the query only matches the tool name", async () => {
    render(
      <SessionToolGroup item={item} isActive={false} searchQuery="bash" />,
    );
    expect(screen.queryByRole("region")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { expanded: false }));
    expect(screen.getByRole("region")).toBeInTheDocument();
  });
});
