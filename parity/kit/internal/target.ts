// Targets are opaque to cases. Only the screen builders make them, and only the driver reads
// what they mean. A target is a role and an accessible name, optionally inside another target.

export type TargetSpec =
  | { readonly kind: "role"; readonly role: string; readonly name: string; readonly within?: TargetSpec }
  | { readonly kind: "field"; readonly name: string; readonly within?: TargetSpec }
  | { readonly kind: "text"; readonly name: string; readonly within?: TargetSpec };

export interface Target {
  readonly __brand: "Target";
}

const specs = new WeakMap<object, TargetSpec>();

export function describe(spec: TargetSpec): string {
  const self =
    spec.kind === "role" ? `${spec.role} "${spec.name}"` : spec.kind === "field" ? `field "${spec.name}"` : `text "${spec.name}"`;
  return spec.within ? `${self} in ${describe(spec.within)}` : self;
}

export function makeTarget(spec: TargetSpec): Target {
  const t = Object.freeze({ toString: () => describe(spec) });
  specs.set(t, spec);
  return t as unknown as Target;
}

export function specOf(t: Target): TargetSpec {
  const spec = specs.get(t as object);
  if (!spec) throw new Error("not a target built by the screen map");
  return spec;
}
