// The in-page probe. One self-contained function, injected with Function.prototype.toString, so it
// must not refer to anything outside its own body. It reads the page the way assistive technology
// does: roles, accessible names, values and states, never DOM structure or CSS classes.
import { TRANSIENT_UI } from "../../../volatile.ts";
import type { TargetSpec } from "../target.ts";

export type ProbeCmd =
  | { readonly cmd: "tree"; readonly transient: readonly string[] }
  | { readonly cmd: "resolve"; readonly target: TargetSpec; readonly element?: boolean; readonly act?: boolean }
  | { readonly cmd: "install" }
  | { readonly cmd: "announcements" };

export interface TreeResult {
  readonly title: string;
  readonly scheme: "light" | "dark";
  readonly focused: string | null;
  readonly text: string;
}

export type ResolveResult =
  | { readonly state: "none" | "ambiguous"; readonly count: number; readonly why?: string }
  | {
      readonly state: "ok" | "zero-size" | "disabled" | "covered";
      readonly count: 1;
      readonly x: number;
      readonly y: number;
      readonly w: number;
      readonly h: number;
      readonly coveredBy?: string;
      readonly element?: unknown;
    };

export function probe(c: ProbeCmd): unknown {
  const norm = (s: string | null | undefined) => (s || "").replace(/\s+/g, " ").trim();
  const LEAF = new Set([
    "button", "link", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "checkbox", "radio",
    "switch", "heading", "img", "textbox", "searchbox", "combobox", "slider", "spinbutton", "progressbar",
    "separator", "meter",
  ]);
  const FROM_CONTENT = new Set([
    "button", "link", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "option", "checkbox", "radio",
    "switch", "heading", "cell", "columnheader", "rowheader", "tooltip", "treeitem",
  ]);
  const TRANSPARENT = new Set(["presentation", "none", "generic"]);
  // Declared transient subtrees (volatile.ts) are left out of the tree, never out of targeting.
  const transient = c.cmd === "tree" && c.transient.length ? c.transient.join(", ") : null;

  const hiddenSelf = (el: Element): boolean => {
    if ((el as HTMLElement).hidden) return true;
    if (el.getAttribute("aria-hidden") === "true") return true;
    if (el.hasAttribute("inert")) return true;
    const cs = getComputedStyle(el);
    return cs.display === "none" || cs.visibility === "hidden" || cs.visibility === "collapse";
  };
  const hiddenDeep = (el: Element): boolean => {
    for (let e: Element | null = el; e && e !== document.documentElement; e = e.parentElement) if (hiddenSelf(e)) return true;
    return false;
  };

  const roleOf = (el: Element): string | null => {
    const explicit = el.getAttribute("role");
    if (explicit) return explicit.trim().split(/\s+/)[0];
    const tag = el.tagName;
    switch (tag) {
      case "BUTTON": case "SUMMARY": return "button";
      case "A": return el.hasAttribute("href") ? "link" : null;
      case "INPUT": {
        const t = ((el as HTMLInputElement).type || "text").toLowerCase();
        if (t === "hidden") return null;
        if (t === "checkbox") return "checkbox";
        if (t === "radio") return "radio";
        if (t === "range") return "slider";
        if (t === "number") return "spinbutton";
        if (t === "search") return "searchbox";
        if (t === "button" || t === "submit" || t === "reset" || t === "image") return "button";
        return "textbox";
      }
      case "TEXTAREA": return "textbox";
      case "SELECT": return (el as HTMLSelectElement).multiple || (el as HTMLSelectElement).size > 1 ? "listbox" : "combobox";
      case "OPTION": return "option";
      case "H1": case "H2": case "H3": case "H4": case "H5": case "H6": return "heading";
      case "DIALOG": return "dialog";
      case "NAV": return "navigation";
      case "MAIN": return "main";
      case "ASIDE": return "complementary";
      case "UL": case "OL": case "MENU": return "list";
      case "LI": return "listitem";
      case "TABLE": return "table";
      case "TR": return "row";
      case "TD": return "cell";
      case "TH": return "columnheader";
      case "IMG": return (el as HTMLImageElement).alt === "" ? null : "img";
      case "PROGRESS": return "progressbar";
      case "OUTPUT": return "status";
      case "FIELDSET": return "group";
      case "HR": return "separator";
      case "SECTION": return el.hasAttribute("aria-label") || el.hasAttribute("aria-labelledby") ? "region" : null;
      case "FORM": return el.hasAttribute("aria-label") || el.hasAttribute("aria-labelledby") ? "form" : null;
      default: return null;
    }
  };

  // Name from content: visible text, or the labels of icon-only content.
  const textOf = (el: Element): string => {
    if (hiddenSelf(el)) return "";
    const label = el.getAttribute("aria-label");
    if (label) return norm(label);
    if (el.tagName === "IMG") return norm((el as HTMLImageElement).alt);
    const inner = norm((el as HTMLElement).innerText);
    if (inner) return inner;
    const parts: string[] = [];
    el.querySelectorAll("[aria-label], img[alt], [title]").forEach((d) => {
      if (hiddenDeep(d)) return;
      parts.push(norm(d.getAttribute("aria-label") || (d as HTMLImageElement).alt || d.getAttribute("title")));
    });
    return norm(parts.join(" "));
  };

  const nameOf = (el: Element, role: string | null): string => {
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const n = norm(by.split(/\s+/).map((id) => {
        const t = document.getElementById(id);
        return t ? norm(t.getAttribute("aria-label") || (t as HTMLElement).innerText || t.textContent) : "";
      }).join(" "));
      if (n) return n;
    }
    const label = el.getAttribute("aria-label");
    if (label && norm(label)) return norm(label);
    const tag = el.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") {
      const input = el as HTMLInputElement;
      if (tag === "INPUT" && ["button", "submit", "reset"].includes(input.type)) return norm(input.value);
      const labels = input.labels ? Array.from(input.labels).map((l) => norm(l.innerText || l.textContent)) : [];
      if (labels.join("")) return norm(labels.join(" "));
      if (el.getAttribute("title")) return norm(el.getAttribute("title"));
      if (input.placeholder) return norm(input.placeholder);
      return "";
    }
    if (tag === "IMG") return norm((el as HTMLImageElement).alt);
    if (role && FROM_CONTENT.has(role)) {
      const t = textOf(el);
      if (t) return t;
    }
    return norm(el.getAttribute("title"));
  };

  const valueOf = (el: Element, role: string): string | null => {
    const tag = el.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") {
      const input = el as HTMLInputElement;
      if (input.type === "password") return null;
      if (role === "checkbox" || role === "radio" || role === "button") return null;
      return input.value;
    }
    if (tag === "SELECT") {
      const s = el as HTMLSelectElement;
      return s.selectedOptions.length ? norm(s.selectedOptions[0].textContent) : "";
    }
    if (role === "combobox" || role === "slider" || role === "spinbutton" || role === "progressbar") {
      const v = el.getAttribute("aria-valuetext") || el.getAttribute("aria-valuenow");
      if (v !== null) return norm(v);
      if (role === "combobox") return norm((el as HTMLElement).innerText);
    }
    return null;
  };

  const statesOf = (el: Element, role: string): string[] => {
    const s: string[] = [];
    const a = (n: string) => el.getAttribute(n);
    if ((el as HTMLButtonElement).disabled || a("aria-disabled") === "true") s.push("disabled");
    const checked = a("aria-checked") ?? (el.tagName === "INPUT" && (role === "checkbox" || role === "radio") ? String((el as HTMLInputElement).checked) : null);
    if (checked !== null) s.push(checked === "true" ? "checked" : checked === "mixed" ? "mixed" : "unchecked");
    if (a("aria-expanded") !== null) s.push(a("aria-expanded") === "true" ? "expanded" : "collapsed");
    if (a("aria-selected") === "true") s.push("selected");
    if (a("aria-pressed") !== null) s.push(a("aria-pressed") === "true" ? "pressed" : "not-pressed");
    const cur = a("aria-current");
    if (cur && cur !== "false") s.push(`current=${cur}`);
    if (a("aria-invalid") === "true") s.push("invalid");
    if ((el as HTMLInputElement).required || a("aria-required") === "true") s.push("required");
    if ((el as HTMLInputElement).readOnly || a("aria-readonly") === "true") s.push("readonly");
    if (el.tagName === "INPUT" && (el as HTMLInputElement).type === "password") s.push("password");
    if (document.activeElement === el) s.push("focused");
    return s;
  };

  const line = (el: Element, role: string): string => {
    const name = nameOf(el, role);
    const value = valueOf(el, role);
    const states = statesOf(el, role);
    return `${role}${name ? ` ${JSON.stringify(name)}` : ""}${value !== null ? ` = ${JSON.stringify(value)}` : ""}${states.length ? ` [${states.join(" ")}]` : ""}`;
  };

  // Visits the accessibility tree in document order. `visit` returns false to skip a subtree.
  const walk = (root: Element, visit: (node: Node, role: string | null, depth: number) => boolean | void, depth = 0) => {
    for (const child of Array.from(root.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        visit(child, null, depth);
        continue;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) continue;
      const el = child as Element;
      if (el.tagName === "SCRIPT" || el.tagName === "STYLE" || el.tagName === "TEMPLATE" || el.tagName === "NOSCRIPT") continue;
      if (hiddenSelf(el)) continue;
      if (transient && el.matches(transient)) continue;
      const role = roleOf(el);
      const shown = role !== null && !TRANSPARENT.has(role);
      const descend = visit(el, shown ? role : null, depth);
      if (descend === false) continue;
      if (shown && LEAF.has(role!)) continue;
      walk(el, visit, shown ? depth + 1 : depth);
    }
  };

  const describeEl = (el: Element | null): string | null => {
    if (!el || el === document.body || el === document.documentElement) return null;
    const role = roleOf(el) || el.tagName.toLowerCase();
    const name = nameOf(el, role);
    return `${role}${name ? ` ${JSON.stringify(name)}` : ""}`;
  };

  const matches = (el: Element, role: string | null, t: TargetSpec): boolean => {
    if (t.kind === "role") return role === t.role && nameOf(el, role) === t.name;
    if (t.kind === "field") return (role === "textbox" || role === "combobox" || role === "searchbox" || role === "spinbutton") && nameOf(el, role) === t.name;
    return false;
  };

  const findAll = (t: TargetSpec): { found: Element[]; why?: string } => {
    let scope: Element = document.body;
    if (t.within) {
      const outer = findAll(t.within);
      if (outer.found.length !== 1) return { found: [], why: `container ${outer.found.length === 0 ? "not found" : "ambiguous"}` };
      scope = outer.found[0];
    }
    const found: Element[] = [];
    if (t.kind === "text") {
      walk(scope, (node) => {
        if (node.nodeType !== Node.ELEMENT_NODE) return;
        const el = node as Element;
        if (norm((el as HTMLElement).innerText) !== t.name) return;
        const deeper = Array.from(el.children).some((ch) => !hiddenSelf(ch) && norm((ch as HTMLElement).innerText) === t.name);
        if (!deeper) found.push(el);
      });
      return { found };
    }
    walk(scope, (node, role) => {
      if (node.nodeType !== Node.ELEMENT_NODE) return;
      const el = node as Element;
      if (matches(el, roleOf(el), t)) found.push(el);
      return role === null || !LEAF.has(role);
    });
    return { found };
  };

  if (c.cmd === "tree") {
    const lines: string[] = [];
    walk(document.body, (node, role, depth) => {
      const pad = "  ".repeat(depth);
      if (node.nodeType === Node.TEXT_NODE) {
        const t = norm(node.textContent);
        if (t) lines.push(`${pad}text ${JSON.stringify(t)}`);
        return;
      }
      if (role) lines.push(pad + line(node as Element, role));
    });
    const result: TreeResult = {
      title: document.title,
      scheme: matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
      focused: describeEl(document.activeElement),
      text: lines.join("\n"),
    };
    return result;
  }

  if (c.cmd === "resolve") {
    const { found, why } = findAll(c.target);
    if (found.length !== 1) return { state: found.length === 0 ? "none" : "ambiguous", count: found.length, why } satisfies ResolveResult;
    const el = found[0] as HTMLElement;
    // Only a step that acts may scroll; observing must not change what is on screen.
    if (c.act) el.scrollIntoView({ block: "nearest", inline: "nearest" });
    const r = el.getBoundingClientRect();
    const base = { count: 1 as const, x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height, element: c.element ? el : undefined };
    if (r.width === 0 || r.height === 0) return { ...base, state: "zero-size" } satisfies ResolveResult;
    if ((el as HTMLButtonElement).disabled || el.getAttribute("aria-disabled") === "true") return { ...base, state: "disabled" } satisfies ResolveResult;
    const top = document.elementFromPoint(base.x, base.y);
    if (top && top !== el && !el.contains(top)) return { ...base, state: "covered", coveredBy: describeEl(top) ?? top.tagName } satisfies ResolveResult;
    return { ...base, state: "ok" } satisfies ResolveResult;
  }

  // Toasts and live-region messages are transient, so they are logged as they appear.
  const w = window as unknown as { __parityAnn?: string[]; __parityObs?: MutationObserver };
  if (c.cmd === "install") {
    if (w.__parityObs) return true;
    w.__parityAnn = [];
    const seen = new WeakSet<Element>();
    const record = (el: Element) => {
      if (seen.has(el)) return;
      seen.add(el);
      setTimeout(() => {
        const t = textOf(el);
        if (t) w.__parityAnn!.push(t);
      }, 80);
    };
    const scan = (n: Node) => {
      if (n.nodeType !== Node.ELEMENT_NODE) return;
      const el = n as Element;
      const pick = (e: Element) => e.matches("[data-sonner-toast], [role=alert], [role=status]") && record(e);
      pick(el);
      el.querySelectorAll("[data-sonner-toast], [role=alert], [role=status]").forEach(pick);
    };
    w.__parityObs = new MutationObserver((ms) => ms.forEach((m) => m.addedNodes.forEach(scan)));
    w.__parityObs.observe(document.body, { childList: true, subtree: true });
    return true;
  }

  if (c.cmd === "announcements") {
    const out = w.__parityAnn ? w.__parityAnn.splice(0) : [];
    return out;
  }
  return null;
}

/** The tree command, with the declared transient subtrees left out. */
export const TREE_CMD: ProbeCmd = { cmd: "tree", transient: TRANSIENT_UI.map((r) => r.selector) };

/** The script body for WebDriver's execute endpoints. */
export const PROBE_SCRIPT = `return (${probe.toString()})(arguments[0]);`;
