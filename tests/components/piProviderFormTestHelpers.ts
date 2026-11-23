import { fireEvent, screen } from "@testing-library/react";
import { parse as parseYaml } from "yaml";

// omp 的编辑框按 models.yml 显示 `providers.<标识>` 这一个节点
export function ompNode(editor: HTMLTextAreaElement) {
  const { providers } = parseYaml(editor.value) as {
    providers: Record<string, Record<string, any>>;
  };
  return Object.values(providers)[0];
}

export function completeModel(id: string, name = id.trim() || "Model") {
  return {
    id,
    name,
    reasoning: false,
    input: ["text"],
    contextWindow: 128_000,
    maxTokens: 16_384,
  };
}

// 请求头、模型自动发现和接口兼容性收在默认折叠的「高级选项」里
export function openAdvancedOptions() {
  fireEvent.click(
    screen.getByRole("button", { name: "providerForm.advancedOptionsToggle" }),
  );
}
