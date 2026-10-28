import type { OmpModelManagementState } from "@/lib/api/omp";

/** Display-only exact references; intentionally does not resolve native fuzzy patterns. */
export function summarizeOmpReferences(state: OmpModelManagementState) {
  const known = new Set(
    state.models.map((model) => `${model.provider}/${model.id}`),
  );
  const byModel = new Map<string, string[]>();
  const unresolved: string[] = [];
  const referenceEntries: {
    section: "modelRoles" | "task.agentModelOverrides";
    key: string;
    targets: string[];
  }[] = [];
  const thinkingLevels = [
    "off",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
    "max",
    "auto",
    "inherit",
  ];
  const roleKey = (reference: string) => {
    const key =
      reference === "*"
        ? "default"
        : reference.startsWith("@")
          ? reference.slice(1)
          : reference.startsWith("pi/")
            ? reference.slice(3)
            : null;
    return key !== null &&
      Object.prototype.hasOwnProperty.call(state.modelRoles, key)
      ? key
      : null;
  };
  const resolve = (
    value: string | string[],
    visited: Set<string>,
    targets: Set<string>,
    unknown: Set<string>,
  ) => {
    for (const raw of Array.isArray(value) ? value : [value]) {
      if (known.has(raw)) {
        targets.add(raw);
        continue;
      }
      for (const candidate of raw
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean)) {
        if (known.has(candidate)) {
          targets.add(candidate);
          continue;
        }
        const exactRole = roleKey(candidate);
        const colon = candidate.lastIndexOf(":");
        const suffix = colon >= 0 ? candidate.slice(colon + 1) : "";
        const hasThinking =
          suffix.length >= 2 &&
          thinkingLevels.filter((level) => level.startsWith(suffix)).length ===
            1;
        const bare =
          exactRole === null && hasThinking
            ? candidate.slice(0, colon)
            : candidate;
        if (known.has(bare)) {
          targets.add(bare);
          continue;
        }
        const role = exactRole ?? roleKey(bare);
        if (role !== null && !visited.has(role) && visited.size < 64) {
          const next = new Set(visited);
          next.add(role);
          resolve(state.modelRoles[role], next, targets, unknown);
        } else unknown.add(candidate);
      }
    }
  };
  for (const [section, entries] of [
    ["modelRoles", state.modelRoles],
    ["task.agentModelOverrides", state.agentModelOverrides],
  ] as const) {
    for (const [key, value] of Object.entries(entries)) {
      const targets = new Set<string>();
      const unknown = new Set<string>();
      const description = `${section}.${key} → ${Array.isArray(value) ? value.join(" | ") : value}`;
      resolve(
        value,
        new Set(section === "modelRoles" ? [key] : []),
        targets,
        unknown,
      );
      referenceEntries.push({ section, key, targets: [...targets] });
      for (const target of targets)
        byModel.set(target, [...(byModel.get(target) ?? []), description]);
      if (unknown.size)
        unresolved.push(`${description} (${[...unknown].join(" | ")})`);
    }
  }
  return { byModel, unresolved, entries: referenceEntries };
}
