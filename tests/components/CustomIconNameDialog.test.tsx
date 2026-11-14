import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { CustomIconNameDialog } from "@/components/CustomIconNameDialog";
import { pngDataUri } from "../fixtures/customIcon";

describe("CustomIconNameDialog", () => {
  // 选择器渲染在供应商表单里：起名框的提交事件会沿 React 组件树穿过 Portal 冒泡，
  // 不拦住就会顺带提交外层表单，提前保存供应商
  it("does not submit the provider form it is rendered inside", () => {
    const onOuterSubmit = vi.fn((e: React.FormEvent) => e.preventDefault());
    const onConfirm = vi.fn();
    render(
      <form onSubmit={onOuterSubmit}>
        <CustomIconNameDialog
          icon={pngDataUri(96, 96)}
          initialName="Relay"
          title="title"
          confirmText="confirm"
          onConfirm={onConfirm}
          onCancel={() => {}}
        />
      </form>,
    );

    fireEvent.click(screen.getByText("confirm"));
    expect(onConfirm).toHaveBeenCalledWith("Relay");
    expect(onOuterSubmit).not.toHaveBeenCalled();
  });
});
