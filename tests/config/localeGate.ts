/**
 * Structural locale gate helpers.
 *
 * The gate answers "does every locale carry exactly the keys the reference
 * locale defines for these namespaces?" — it deliberately avoids heuristics on
 * translated *values*. A value regex (e.g. `/\bPi\b/`) changes meaning the
 * moment copy is reworded, which is how locale gates quietly rot.
 *
 * Selection is therefore purely structural:
 *   - closed namespace prefixes (must end at a `.` segment boundary), and
 *   - an explicit, non-empty list of keys that live outside those namespaces.
 */

export type LocaleTree = Record<string, unknown>;

/** Flattened `dotted.key -> value` view of a locale tree. */
export type FlatTranslations = Map<string, string>;

export function flattenStrings(
  value: unknown,
  path: string[] = [],
  result: FlatTranslations = new Map(),
): FlatTranslations {
  if (typeof value === "string") {
    result.set(path.join("."), value);
  } else if (typeof value === "object" && value !== null) {
    for (const [key, child] of Object.entries(value)) {
      flattenStrings(child, [...path, key], result);
    }
  }
  return result;
}

/**
 * Pick the keys every locale is required to carry.
 *
 * @param reference flattened reference locale (`en`)
 * @param namespaces closed prefixes, e.g. `["remote."]`
 * @param extraKeys keys the gate covers that sit outside `namespaces`
 */
export function selectCoveredKeys(
  reference: FlatTranslations,
  namespaces: string[],
  extraKeys: string[],
): Set<string> {
  if (namespaces.length === 0) {
    throw new Error("selectCoveredKeys: at least one namespace is required");
  }
  if (extraKeys.length === 0) {
    throw new Error(
      "selectCoveredKeys: at least one extra key is required, otherwise the gate covers nothing",
    );
  }

  for (const namespace of namespaces) {
    if (!namespace.endsWith(".")) {
      throw new Error(
        `selectCoveredKeys: namespace "${namespace}" must be a closed prefix ending with "."`,
      );
    }
  }

  const covered = new Set<string>();
  for (const key of reference.keys()) {
    if (namespaces.some((namespace) => key.startsWith(namespace))) {
      covered.add(key);
    }
  }

  for (const key of extraKeys) {
    if (!reference.has(key)) {
      // Silently dropping a typo'd extra key would widen the hole the gate is
      // supposed to close.
      throw new Error(
        `selectCoveredKeys: extra key "${key}" is absent from the reference locale`,
      );
    }
    covered.add(key);
  }

  return covered;
}

/** Covered keys the locale does not define (missing, empty, or shadowed by an object). */
export function findMissing(
  tree: LocaleTree,
  covered: Iterable<string>,
): string[] {
  const translations = flattenStrings(tree);
  return [...covered].filter((key) => !translations.has(key)).sort();
}

/** Keys the locale defines under `namespaces` that the reference locale does not. */
export function findOrphan(
  tree: LocaleTree,
  reference: FlatTranslations,
  namespaces: string[],
): string[] {
  const translations = flattenStrings(tree);
  return [...translations.keys()]
    .filter(
      (key) =>
        !reference.has(key) &&
        namespaces.some((namespace) => key.startsWith(namespace)),
    )
    .sort();
}
