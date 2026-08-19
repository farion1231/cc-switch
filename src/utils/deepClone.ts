export function deepClone<T>(value: T): T {
  if (typeof globalThis.structuredClone === "function") {
    return globalThis.structuredClone(value);
  }

  return deepCloneFallback(value);
}

function deepCloneFallback<T>(value: T): T {
  if (value === null || typeof value !== "object") return value;
  if (value instanceof Date) return new Date(value.getTime()) as T;
  if (Array.isArray(value)) {
    return value.map((item) => deepCloneFallback(item)) as T;
  }

  const cloned = {} as T;
  Object.keys(value).forEach((key) => {
    // `cloned["__proto__"] = …` 走的是 setter，会替换 cloned 自己的原型而不是
    // 新增一个自有属性。这不会污染全局 `Object.prototype`（已实测），但会让克隆体
    // 凭空读得到源对象里那些键，是个难查的幽灵属性。`structuredClone` 把
    // `__proto__` 当普通数据键原样保留，两条路径的行为因此不一致——跳过它，
    // 让 fallback 与主路径对齐。
    if (key === "__proto__") return;
    cloned[key as keyof T] = deepCloneFallback(value[key as keyof T]);
  });
  return cloned;
}
