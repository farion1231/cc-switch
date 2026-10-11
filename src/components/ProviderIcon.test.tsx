import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ProviderIcon } from "./ProviderIcon";

// mock 本地图标资产读取：missing-key → null（资产缺失），found-key → data URL
const readProviderIconMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/providerIcons", () => ({
  readProviderIcon: (...args: unknown[]) => readProviderIconMock(...args),
  saveProviderIcon: vi.fn(),
  deleteProviderIcon: vi.fn(),
  pickProviderIconFile: vi.fn(),
}));

describe("ProviderIcon custom icon rendering (issue #8117)", () => {
  it("falls back to initials when the local asset is missing (no dead <img>)", async () => {
    readProviderIconMock.mockResolvedValue(null);

    const { container } = render(
      <ProviderIcon
        icon="ghost-provider-ab12cd34.png"
        iconSource="local"
        name="Ghost Provider"
      />,
    );

    // 资产缺失：不渲染引用资产键的死链 <img>，静默回落首字母
    await waitFor(() => {
      expect(container.querySelector("img")).toBeNull();
    });
    expect(screen.getByText("GP")).toBeInTheDocument();
    expect(readProviderIconMock).toHaveBeenCalledWith(
      "ghost-provider-ab12cd34.png",
    );
  });

  it("renders a local asset as <img> with the resolved data URL", async () => {
    readProviderIconMock.mockResolvedValue("data:image/png;base64,QUJD");

    const { container } = render(
      <ProviderIcon
        icon="myprovider-1234abcd.png"
        iconSource="local"
        name="My Provider"
      />,
    );

    await waitFor(() => {
      const img = container.querySelector("img");
      expect(img).not.toBeNull();
      expect(img).toHaveAttribute("src", "data:image/png;base64,QUJD");
    });
    // 命中资产时不再渲染首字母 fallback
    expect(screen.queryByText("MP")).toBeNull();
  });

  it("renders an explicit url source as <img src={icon}>", () => {
    const { container } = render(
      <ProviderIcon
        icon="https://example.com/logo.png"
        iconSource="url"
        name="Remote"
      />,
    );
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    expect(img).toHaveAttribute("src", "https://example.com/logo.png");
    expect(readProviderIconMock).not.toHaveBeenCalled();
  });

  it("keeps legacy builtin behavior when iconSource is absent", () => {
    // 旧数据：icon 为内建名称、无 iconSource → 不触发本地资产解析
    const { container } = render(<ProviderIcon icon="openai" name="OpenAI" />);
    // openai 在内建库中以内联 SVG / URL 呈现，关键是不得调用资产读取
    expect(readProviderIconMock).not.toHaveBeenCalled();
    expect(container).toBeInTheDocument();
  });
});
