import { fireEvent, render, screen } from "@testing-library/react";
import { useForm, type FieldPath, type UseFormReturn } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { BasicFormFields } from "@/components/providers/forms/BasicFormFields";
import { Form } from "@/components/ui/form";
import type { ProviderFormData } from "@/lib/schemas/provider";

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <div data-testid="provider-icon" />,
}));

vi.mock("@/components/IconPicker", () => ({
  IconPicker: () => <div data-testid="icon-picker" />,
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
});
