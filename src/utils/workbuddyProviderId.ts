/**
 * WorkBuddy provider ID 推导。
 *
 * WorkBuddy 的 live 配置（`~/.workbuddy/models.json`）是一份扁平模型数组，
 * 后端按 `(url, apiKey)` 聚合成 provider，并用「网关 host 的主域」作为 provider id
 * （见 `src-tauri/src/workbuddy_config.rs::provider_id_from_url`）。
 *
 * 删除 / isInConfig 判断都按这个 id 匹配，因此前端新增 WorkBuddy 供应商时
 * 必须用同样的规则生成 id，不能用随机 UUID。
 *
 * 本文件是 Rust 侧规则的镜像实现，修改任一侧都必须同步另一侧
 * （`workbuddyProviderId.test.ts` 与 Rust `provider_id_from_url_rules` 用例对齐）。
 */

const FALLBACK_ID = "workbuddy";

/**
 * 由网关地址推导 provider id：取 host 的主域。
 *
 * - `https://api.alpha.test/v1` → `alpha`
 * - `https://api.beta.test/v1` → `beta`
 * - `http://localhost:8080/v1` → `localhost`
 */
export function deriveWorkbuddyProviderId(url: string): string {
  const raw = (url ?? "").trim();
  const schemeIndex = raw.indexOf("://");
  const afterScheme = schemeIndex >= 0 ? raw.slice(schemeIndex + 3) : raw;
  // 去掉 path 与端口
  const host = afterScheme.split("/")[0].split(":")[0];
  const parts = host.split(".");
  const candidate = parts.length >= 2 ? parts[parts.length - 2] : host;
  return candidate || FALLBACK_ID;
}

/**
 * 在已有 provider id 集合中解析出一个不冲突的 WorkBuddy provider id。
 *
 * 与 Rust 侧聚合时的去重规则一致：冲突时依次追加 `-2`、`-3`……
 */
export function resolveWorkbuddyProviderId(
  baseUrl: string,
  existingIds: Iterable<string>,
): string {
  const base = deriveWorkbuddyProviderId(baseUrl);
  const taken = new Set(existingIds);
  if (!taken.has(base)) return base;

  let n = 2;
  while (taken.has(`${base}-${n}`)) {
    n += 1;
  }
  return `${base}-${n}`;
}
