import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useForm, type FieldPath, type UseFormReturn } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { Form } from "@/components/ui/form";
import type { ProviderFormData } from "@/lib/schemas/provider";

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <div data-testid="provider-icon" />,
}));

vi.mock("@/components/IconPicker", () => ({
  IconPicker: ({
    onValueChange,
  }: {
    onValueChange: (icon: string) => void;
  }) => (
    <div data-testid="icon-picker">
      <button type="button" onClick={() => onValueChange("anthropic")}>
        pick-anthropic
      </button>
    </div>
  ),
}));

const defaultValues: Partial<ProviderFormData> = {
  name: "原供应商",
  notes: "原备注",
  websiteUrl: "https://example.com",
  icon: "",
  iconColor: "",
};

function renderBasicForm() {
  let formApi!: UseFormReturn<ProviderFormData>;

  function TestForm() {
    const form = useForm<ProviderFormData>({ defaultValues });
    formApi = form;
    return (
      <Form {...form}>
        <BasicFormFields form={form} />
      </Form>
    );
  }

  const rendered = render(<TestForm />);
  return { ...rendered, getForm: () => formApi, TestForm };
}

describe("BasicFormFields", () => {
  it.each([
    ["provider.name", "name", "原供应商"],
    ["provider.notes", "notes", "原备注"],
    ["provider.websiteUrl", "websiteUrl", "https://example.com"],
  ] as const)(
    "keeps %s composition local until the IME commits",
    (label, fieldName, initialValue) => {
      const { getForm, rerender, TestForm } = renderBasicForm();
      const input = screen.getByLabelText(label);

      fireEvent.compositionStart(input);
      fireEvent.change(input, { target: { value: "中文输入" } });

      expect(input).toHaveValue("中文输入");
      expect(
        getForm().getValues(fieldName as FieldPath<ProviderFormData>),
      ).toBe(initialValue);

      // An unrelated parent render must not replace the browser-owned marked
      // text with React Hook Form's last committed value.
      rerender(<TestForm />);
      expect(input).toHaveValue("中文输入");

      fireEvent.compositionEnd(input, {
        data: "中文输入",
        target: { value: "中文输入" },
      });

      expect(
        getForm().getValues(fieldName as FieldPath<ProviderFormData>),
      ).toBe("中文输入");
    },
  );

  it.each([
    ["provider.name", "name", "原供应商"],
    ["provider.notes", "notes", "原备注"],
    ["provider.websiteUrl", "websiteUrl", "https://example.com"],
  ] as const)(
    "force-commits unfinished %s composition on blur",
    (label, fieldName, initialValue) => {
      const { getForm } = renderBasicForm();
      const input = screen.getByLabelText(label);

      fireEvent.compositionStart(input);
      fireEvent.change(input, { target: { value: "失焦提交" } });

      expect(
        getForm().getValues(fieldName as FieldPath<ProviderFormData>),
      ).toBe(initialValue);

      fireEvent.blur(input);

      expect(
        getForm().getValues(fieldName as FieldPath<ProviderFormData>),
      ).toBe("失焦提交");
    },
  );

  it("mounts the icon picker inside #content-area as a nested FullScreenPanel without covering the sidebar", async () => {
    const onOuterClose = vi.fn();
    let formApi!: UseFormReturn<ProviderFormData>;

    function ShellWithAddProvider({ open }: { open: boolean }) {
      const form = useForm<ProviderFormData>({ defaultValues });
      formApi = form;
      return (
        <div>
          <nav data-testid="sidebar">sidebar</nav>
          <main id="content-area">
            <FullScreenPanel
              isOpen={open}
              title="Add provider"
              onClose={onOuterClose}
            >
              <Form {...form}>
                <BasicFormFields form={form} />
              </Form>
            </FullScreenPanel>
          </main>
        </div>
      );
    }

    const view = render(<ShellWithAddProvider open={false} />);
    view.rerender(<ShellWithAddProvider open />);
    const contentArea = document.getElementById("content-area")!;

    fireEvent.click(screen.getByRole("button", { name: "更换图标" }));

    const heading = within(contentArea).getByRole("heading", {
      name: "选择图标",
    });
    const iconPanel = heading.closest(".bg-app")!;
    expect(contentArea).toContainElement(iconPanel as HTMLElement);
    expect(iconPanel).toHaveClass("absolute", "inset-0");
    expect(iconPanel).not.toHaveClass("fixed", "w-screen");

    // ESC closes only the icon picker panel, leaving the outer Add provider panel open
    fireEvent.keyDown(document.body, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "选择图标" })).toBeNull(),
    );
    expect(onOuterClose).not.toHaveBeenCalled();

    // Reopen and pick an icon: updates form values and closes the picker panel
    fireEvent.click(screen.getByRole("button", { name: "更换图标" }));
    fireEvent.click(screen.getByRole("button", { name: "pick-anthropic" }));

    expect(formApi.getValues("icon")).toBe("anthropic");
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "选择图标" })).toBeNull(),
    );
    expect(onOuterClose).not.toHaveBeenCalled();
  });
});
