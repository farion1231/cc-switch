import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { IconPicker } from "@/components/IconPicker";
import { pngDataUri } from "../fixtures/customIcon";

const normalizeIconFile = vi.hoisted(() => vi.fn());
vi.mock("@/utils/customIcon", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/utils/customIcon")>()),
  normalizeIconFile,
}));

const CUSTOM = pngDataUri(96, 96);

/** 上传一个文件，返回让处理完成的函数 */
function startUpload(): (icon: string) => Promise<void> {
  let resolve!: (icon: string) => void;
  normalizeIconFile.mockReturnValueOnce(
    new Promise<string>((r) => {
      resolve = r;
    }),
  );
  fireEvent.change(screen.getByTestId("icon-upload-input"), {
    target: { files: [new File(["x"], "logo.png", { type: "image/png" })] },
  });
  return async (icon) => {
    await act(async () => resolve(icon));
  };
}

describe("IconPicker custom icon", () => {
  beforeEach(() => normalizeIconFile.mockReset());

  it("shows the uploaded icon and clears it on remove", () => {
    const onValueChange = vi.fn();
    render(<IconPicker value={CUSTOM} onValueChange={onValueChange} />);

    expect(screen.getByText("iconPicker.customIcon")).toBeInTheDocument();
    fireEvent.click(screen.getByText("iconPicker.removeCustom"));
    expect(onValueChange).toHaveBeenCalledWith("");
  });

  it("hides the custom row for built-in icons", () => {
    render(<IconPicker value="claude" onValueChange={() => {}} />);
    expect(screen.queryByText("iconPicker.customIcon")).not.toBeInTheDocument();
    expect(screen.getByTestId("icon-upload-input")).toHaveAttribute(
      "accept",
      "image/png,image/jpeg,image/webp,image/svg+xml",
    );
  });

  it("applies a finished upload", async () => {
    const onValueChange = vi.fn();
    render(<IconPicker value="claude" onValueChange={onValueChange} />);
    const finish = startUpload();
    await finish(CUSTOM);
    expect(onValueChange).toHaveBeenLastCalledWith(CUSTOM);
  });

  it("drops an upload that finishes after the user removed the icon", async () => {
    const onValueChange = vi.fn();
    render(<IconPicker value={CUSTOM} onValueChange={onValueChange} />);
    const finish = startUpload();
    fireEvent.click(screen.getByText("iconPicker.removeCustom"));
    await finish(pngDataUri(64, 64));
    expect(onValueChange.mock.calls).toEqual([[""]]);
  });

  it("drops an upload that finishes after the user picked a built-in icon", async () => {
    const onValueChange = vi.fn();
    render(<IconPicker value="claude" onValueChange={onValueChange} />);
    const finish = startUpload();
    fireEvent.click(screen.getAllByTitle("DeepSeek")[0].closest("button")!);
    await finish(CUSTOM);
    expect(onValueChange.mock.calls).toEqual([["deepseek"]]);
  });

  it("drops an upload left running when the picker was closed and reopened", async () => {
    const onValueChange = vi.fn();
    const first = render(
      <IconPicker value="claude" onValueChange={onValueChange} />,
    );
    const finish = startUpload();
    first.unmount();

    render(<IconPicker value="claude" onValueChange={onValueChange} />);
    fireEvent.click(screen.getAllByTitle("DeepSeek")[0].closest("button")!);
    await finish(CUSTOM);
    expect(onValueChange.mock.calls).toEqual([["deepseek"]]);
  });

  it("lets a newer upload win over an older one", async () => {
    const onValueChange = vi.fn();
    render(<IconPicker value="claude" onValueChange={onValueChange} />);
    const finishFirst = startUpload();
    const finishSecond = startUpload();
    const second = pngDataUri(64, 64);
    await finishSecond(second);
    await finishFirst(CUSTOM);
    expect(onValueChange.mock.calls).toEqual([[second]]);
  });
});
