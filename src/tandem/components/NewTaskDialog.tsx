import { useState } from "react";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { containsStructuredCredential } from "../taskValidation";
import type { CreateTaskInput } from "../types";

const EMPTY_INPUT: CreateTaskInput = {
  projectName: "",
  projectRootPath: "",
  title: "",
  originalInstruction: "",
};

const validate = (input: CreateTaskInput): string | null => {
  if (Object.values(input).some((value) => !value.trim())) {
    return "请填写所有字段";
  }
  if (Array.from(input.title.trim()).length > 120) {
    return "任务标题不能超过 120 个字符";
  }
  if (Array.from(input.originalInstruction.trim()).length > 20_000) {
    return "原始指令不能超过 20000 个字符";
  }
  if (
    containsStructuredCredential(input.title) ||
    containsStructuredCredential(input.originalInstruction)
  ) {
    return "请移除结构化明文凭据";
  }
  return null;
};

export function NewTaskDialog({
  submitting,
  onCreate,
}: {
  submitting: boolean;
  onCreate: (input: CreateTaskInput) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [input, setInput] = useState(EMPTY_INPUT);
  const [error, setError] = useState<string | null>(null);

  const update = (field: keyof CreateTaskInput, value: string) => {
    setInput((current) => ({ ...current, [field]: value }));
    setError(null);
  };

  const submit = async () => {
    const validationError = validate(input);
    if (validationError) {
      setError(validationError);
      return;
    }
    const trimmed = Object.fromEntries(
      Object.entries(input).map(([key, value]) => [key, value.trim()]),
    ) as unknown as CreateTaskInput;
    try {
      await onCreate(trimmed);
      setInput(EMPTY_INPUT);
      setError(null);
      setOpen(false);
    } catch {
      // The page owns the redacted toast; keep this dialog open for correction.
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button size="sm">
          <Plus className="h-4 w-4" aria-hidden="true" />
          新建任务
        </Button>
      </DialogTrigger>
      <DialogContent
        overlayClassName="backdrop-blur-none"
        className="max-h-[calc(100vh-2rem)] overflow-y-auto sm:max-w-lg"
      >
        <DialogHeader>
          <DialogTitle>新建任务</DialogTitle>
          <DialogDescription>
            填写项目与任务信息。原始指令仅在此处显示。
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          <div className="grid gap-2">
            <Label htmlFor="tandem-project-name">项目名称</Label>
            <Input
              id="tandem-project-name"
              value={input.projectName}
              onChange={(event) => update("projectName", event.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="tandem-project-path">项目路径</Label>
            <Input
              id="tandem-project-path"
              value={input.projectRootPath}
              onChange={(event) =>
                update("projectRootPath", event.target.value)
              }
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="tandem-task-title">任务标题</Label>
            <Input
              id="tandem-task-title"
              value={input.title}
              onChange={(event) => update("title", event.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="tandem-instruction">原始指令</Label>
            <Textarea
              id="tandem-instruction"
              className="min-h-28 resize-y"
              value={input.originalInstruction}
              onChange={(event) =>
                update("originalInstruction", event.target.value)
              }
            />
          </div>
          {error ? (
            <p className="text-sm text-red-600 dark:text-red-400" role="alert">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button onClick={() => void submit()} disabled={submitting}>
            创建任务
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
