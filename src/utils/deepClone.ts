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
    cloned[key as keyof T] = deepCloneFallback(value[key as keyof T]);
  });
  return cloned;
}
