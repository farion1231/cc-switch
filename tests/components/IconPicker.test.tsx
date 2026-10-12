import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { IconPicker } from "@/components/IconPicker";
import { listCustomIcons, setCustomIcons } from "../msw/state";
import { pngDataUri } from "../fixtures/customIcon";

const normalizeIconFile = vi.hoisted(() => vi.fn());
vi.mock("@/utils/customIcon", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/utils/customIcon")>()),
  normalizeIconFile,
}));

const CUSTOM = pngDataUri(96, 96);
const OTHER = pngDataUri(64, 64);

function withClient(ui: ReactElement) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{ui}</QueryClientProvider>;
}

function renderPicker(props: {
  value?: string;
  onValueChange?: (icon: string) => void;
  defaultName?: string;
}) {
  return render(
    withClient(
      <IconPicker
        value={props.value}
        onValueChange={props.onValueChange ?? (() => {})}
        defaultName={props.defaultName}
      />,
    ),
  );
}

/** 上传一个文件，返回让处理完成的函数 */
function startUpload(fileName = "logo.png"): (icon: string) => Promise<void> {
  let resolve!: (icon: string) => void;
  normalizeIconFile.mockReturnValueOnce(
    new Promise<string>((r) => {
      resolve = r;
    }),
  );
  fireEvent.change(screen.getByTestId("icon-upload-input"), {
    target: { files: [new File(["x"], fileName, { type: "image/png" })] },
  });
  return async (icon) => {
    await act(async () => resolve(icon));
  };
}

describe("IconPicker my icons", () => {
  beforeEach(() => normalizeIconFile.mockReset());

  it("asks for a name after an upload, then saves it to the library and selects it", async () => {
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    const finish = startUpload("My Relay.svg");
    await finish(CUSTOM);

    const name = await screen.findByLabelText("iconPicker.nameLabel");
    expect(name).toHaveValue("My Relay");
    fireEvent.change(name, { target: { value: "  Relay  " } });
    fireEvent.click(screen.getByText("iconPicker.saveAndUse"));

    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith(CUSTOM));
    expect(listCustomIcons()).toEqual([
      expect.objectContaining({ name: "Relay", data: CUSTOM }),
    ]);
  });

  it("keeps nothing when the name dialog is cancelled", async () => {
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    const finish = startUpload();
    await finish(CUSTOM);

    await screen.findByLabelText("iconPicker.nameLabel");
    fireEvent.click(screen.getByText("common.cancel"));

    await waitFor(() =>
      expect(
        screen.queryByLabelText("iconPicker.nameLabel"),
      ).not.toBeInTheDocument(),
    );
    expect(onValueChange).not.toHaveBeenCalled();
    expect(listCustomIcons()).toEqual([]);
  });

  it("lists library icons by name and selects one on click", async () => {
    setCustomIcons([
      { id: "a", name: "Relay A", data: CUSTOM, createdAt: 1 },
      { id: "b", name: "Relay B", data: OTHER, createdAt: 2 },
    ]);
    const onValueChange = vi.fn();
    renderPicker({ value: OTHER, onValueChange });

    fireEvent.click(await screen.findByRole("button", { name: "Relay A" }));
    expect(onValueChange).toHaveBeenCalledWith(CUSTOM);
    // 当前图标就在库里，不再单独显示「当前图标」
    expect(
      screen.queryByTitle("iconPicker.currentIcon"),
    ).not.toBeInTheDocument();
  });

  it("filters library icons together with built-in ones", async () => {
    setCustomIcons([
      { id: "a", name: "Relay A", data: CUSTOM, createdAt: 1 },
      { id: "b", name: "Other", data: OTHER, createdAt: 2 },
    ]);
    renderPicker({ value: "claude" });
    await screen.findByRole("button", { name: "Relay A" });

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "relay" },
    });
    expect(screen.getByRole("button", { name: "Relay A" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Other" })).not.toBeInTheDocument();
  });

  it("offers to save a current icon that is not in the library", async () => {
    renderPicker({ value: CUSTOM, defaultName: "Example AI" });

    expect(screen.getByTitle("iconPicker.currentIcon")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("iconPicker.saveToLibrary"));
    const name = await screen.findByLabelText("iconPicker.nameLabel");
    expect(name).toHaveValue("Example AI");
    fireEvent.click(screen.getByText("iconPicker.saveToLibrary"));

    await waitFor(() =>
      expect(listCustomIcons()).toEqual([
        expect.objectContaining({ name: "Example AI", data: CUSTOM }),
      ]),
    );
  });

  it("renames and deletes library icons without touching the selection", async () => {
    setCustomIcons([{ id: "a", name: "Old", data: CUSTOM, createdAt: 1 }]);
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    await screen.findByRole("button", { name: "Old" });

    fireEvent.click(screen.getByLabelText("iconPicker.rename"));
    const name = await screen.findByLabelText("iconPicker.nameLabel");
    fireEvent.change(name, { target: { value: "New" } });
    fireEvent.click(screen.getByText("common.save"));
    expect(await screen.findByRole("button", { name: "New" })).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("iconPicker.delete"));
    fireEvent.click(await screen.findByRole("button", { name: "iconPicker.delete" }));
    await waitFor(() => expect(listCustomIcons()).toEqual([]));
    expect(onValueChange).not.toHaveBeenCalled();
  });

  it("clears the icon", () => {
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    fireEvent.click(screen.getByText("iconPicker.clear"));
    expect(onValueChange).toHaveBeenCalledWith("");
  });

  it("drops an upload that finishes after the user picked a built-in icon", async () => {
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    const finish = startUpload();
    fireEvent.click(screen.getAllByTitle("DeepSeek")[0].closest("button")!);
    await finish(CUSTOM);
    expect(onValueChange.mock.calls).toEqual([["deepseek"]]);
    expect(
      screen.queryByLabelText("iconPicker.nameLabel"),
    ).not.toBeInTheDocument();
  });

  it("drops an upload left running when the picker was closed and reopened", async () => {
    const first = renderPicker({ value: "claude" });
    const finish = startUpload();
    first.unmount();

    renderPicker({ value: "claude" });
    await finish(CUSTOM);
    expect(
      screen.queryByLabelText("iconPicker.nameLabel"),
    ).not.toBeInTheDocument();
  });

  it("lets a newer upload win over an older one", async () => {
    const onValueChange = vi.fn();
    renderPicker({ value: "claude", onValueChange });
    const finishFirst = startUpload("first.png");
    const finishSecond = startUpload("second.png");
    await finishSecond(OTHER);
    await finishFirst(CUSTOM);

    expect(await screen.findByLabelText("iconPicker.nameLabel")).toHaveValue(
      "second",
    );
    fireEvent.click(screen.getByText("iconPicker.saveAndUse"));
    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith(OTHER));
    expect(onValueChange).toHaveBeenCalledTimes(1);
  });
});
