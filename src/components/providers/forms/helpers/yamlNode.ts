import { parse as parseYaml, stringify as stringifyYaml } from "yaml";

type Table = Record<string, unknown>;

const isTable = (value: unknown): value is Table =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * 累加式 YAML 应用里一家供应商就是 `<section>.<key>` 下的一个节点
 * （MiniMax Code 的 `custom_provider`、omp 的 `providers`）。编辑器按文件里的
 * 位置显示这一个节点，带上外层的两级键。
 */
export function yamlNodeToText(
  section: string,
  key: string,
  value: unknown,
): string {
  return stringifyYaml({ [section]: { [key]: value } });
}

export type YamlNodeError =
  | { kind: "syntax"; detail: string }
  | { kind: "scope" };

/**
 * 读回编辑器里的节点：文档只能有 `<section>.<key>` 这一个节点，供应商键要和
 * 上方的「供应商标识」一致。字段本身由各表单自己的 schema 校验。
 */
export function yamlNodeFromText(
  text: string,
  section: string,
  key: string,
): { ok: true; value: Table } | { ok: false; error: YamlNodeError } {
  let document: unknown;
  try {
    document = parseYaml(text);
  } catch (error) {
    return {
      ok: false,
      error: {
        kind: "syntax",
        detail: error instanceof Error ? error.message : String(error),
      },
    };
  }
  const nodes = isTable(document) ? document[section] : undefined;
  if (
    !isTable(document) ||
    Object.keys(document).length !== 1 ||
    !isTable(nodes) ||
    Object.keys(nodes).length !== 1 ||
    !isTable(nodes[key])
  ) {
    return { ok: false, error: { kind: "scope" } };
  }
  return { ok: true, value: nodes[key] };
}
