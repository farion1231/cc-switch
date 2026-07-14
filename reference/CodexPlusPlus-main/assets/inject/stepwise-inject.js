(() => {
  "use strict";

  const API_KEY = "__codexStepwisePanel";
  const STYLE_ID = "codex-stepwise-panel-style";
  const ROOT_ATTR = "data-codex-stepwise-root";
  const PAYLOAD_ATTR = "data-codex-stepwise-payload";
  const SCRIPT_VERSION = "1.0.0-core";
  const PAGE_BRIDGE = "__codexSessionDeleteBridge";
  const POSITION_KEY = "codex-stepwise-float-position-v1";
  const DIAGNOSTICS_KEY = "codex-stepwise-diagnostics-v1";
  const SCAN_DELAY_MS = 220;
  const STREAM_IDLE_MS = 1300;
  const BRIDGE_TIMEOUT_MS = 26000;
  const MAX_TEXT_LENGTH = 12000;
  const MAX_STEPWISE_ITEMS = 6;
  const MAX_PROMPT_LENGTH = 420;
  const MAX_DIAGNOSTICS = 80;
  const EDITABLE_SUBMIT_DELAY_MS = 120;
  const SUBMIT_RETRY_DELAY_MS = 50;
  const SUBMIT_RETRY_LIMIT = 600;
  const INSTANCE_ID = `${SCRIPT_VERSION}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  let codexAppActionsPromise = null;
  let settingsPromise = null;
  let startupPromise = null;

  const previous = window[API_KEY];
  if (previous && typeof previous.destroy === "function") previous.destroy();
  document.querySelectorAll?.(`[${ROOT_ATTR}="true"]`).forEach((node) => node.remove());
  document.getElementById(STYLE_ID)?.remove();

  const state = {
    observer: null,
    themeObserver: null,
    timer: 0,
    root: null,
    fab: null,
    popover: null,
    open: false,
    activeTab: "next",
    position: null,
    drag: null,
    lastAssistantHash: "",
    lastAssistantAt: 0,
    currentHash: "",
    lastScanStatus: "",
    bridgeCache: new Map(),
    bridgePendingHash: "",
    bridgeStatus: "idle",
    bridgeError: "",
    prompts: [],
    settings: null,
    settingsStatus: "",
    theme: "dark",
    themeMode: "auto",
    scans: 0,
    destroyed: false,
    diagnostics: readDiagnostics(),
  };

  function isCurrentInstance() {
    return !state.destroyed && window[API_KEY]?.instanceId === INSTANCE_ID;
  }

  function normalizeText(value) {
    return String(value || "")
      .replace(/\u00a0/g, " ")
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .replace(/[ \t]{2,}/g, " ")
      .trim();
  }

  function shortText(value, limit = MAX_TEXT_LENGTH) {
    const text = normalizeText(value);
    return text.length > limit ? text.slice(text.length - limit) : text;
  }

  function hashText(value) {
    const text = shortText(value, 4000);
    let hash = 2166136261;
    for (let index = 0; index < text.length; index += 1) {
      hash ^= text.charCodeAt(index);
      hash = Math.imul(hash, 16777619);
    }
    return (hash >>> 0).toString(36);
  }

  function clamp(value, min, max) {
    return Math.min(max, Math.max(min, value));
  }

  function rectSummary(node) {
    const rect = visibleRect(node);
    if (!rect) return null;
    return {
      left: Math.round(rect.left),
      top: Math.round(rect.top),
      right: Math.round(rect.right),
      bottom: Math.round(rect.bottom),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
    };
  }

  function readDiagnostics() {
    try {
      const parsed = JSON.parse(sessionStorage.getItem(DIAGNOSTICS_KEY) || "[]");
      return Array.isArray(parsed) ? parsed.slice(-MAX_DIAGNOSTICS) : [];
    } catch {
      return [];
    }
  }

  function writeDiagnostics() {
    try {
      sessionStorage.setItem(DIAGNOSTICS_KEY, JSON.stringify(state.diagnostics.slice(-MAX_DIAGNOSTICS)));
    } catch {}
  }

  function pushDiagnostic(event, details = {}) {
    state.diagnostics.push({
      at: new Date().toISOString(),
      instanceId: INSTANCE_ID,
      event,
      details,
    });
    if (state.diagnostics.length > MAX_DIAGNOSTICS) {
      state.diagnostics.splice(0, state.diagnostics.length - MAX_DIAGNOSTICS);
    }
    writeDiagnostics();
  }

  function visibleRect(node) {
    if (!(node instanceof Element)) return null;
    const rect = node.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    return rect;
  }

  function visibleElement(node) {
    const rect = visibleRect(node);
    return Boolean(rect && rect.width > 20 && rect.height > 10 && rect.bottom > 0 && rect.top < window.innerHeight);
  }

  function parseRgb(color) {
    const match = String(color || "").match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?/i);
    if (!match) return null;
    return {
      r: Number(match[1]),
      g: Number(match[2]),
      b: Number(match[3]),
      a: match[4] === undefined ? 1 : Number(match[4]),
    };
  }

  function luminance(rgb) {
    if (!rgb) return 0;
    return 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
  }

  function detectCodexTheme() {
    const rootClass = document.documentElement.classList;
    if (rootClass.contains("electron-dark") || rootClass.contains("theme-dark")) return "dark";
    if (rootClass.contains("electron-light") || rootClass.contains("theme-light")) return "light";

    const bodyClass = document.body?.classList;
    if (bodyClass?.contains("electron-dark") || bodyClass?.contains("theme-dark")) return "dark";
    if (bodyClass?.contains("electron-light") || bodyClass?.contains("theme-light")) return "light";

    const explicitTokens = [
      document.documentElement.getAttribute("data-theme"),
      document.documentElement.getAttribute("color-scheme"),
      document.body?.getAttribute("data-theme"),
      getComputedStyle(document.documentElement).colorScheme,
    ].join(" ");
    if (/\bdark\b/i.test(explicitTokens)) return "dark";
    if (/\blight\b/i.test(explicitTokens)) return "light";

    const candidates = [
      document.querySelector(".thread-scroll-container"),
      document.querySelector("main"),
      document.body,
      document.documentElement,
    ].filter(Boolean);
    for (const node of candidates) {
      const color = getComputedStyle(node).backgroundColor;
      const rgb = parseRgb(color);
      if (rgb && rgb.a > 0.05 && luminance(rgb) > 5) return luminance(rgb) < 128 ? "dark" : "light";
    }
    return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function syncTheme() {
    localStorage.removeItem("codex-stepwise-theme-mode-v1");
    state.themeMode = "auto";
    state.theme = detectCodexTheme();
    state.root?.setAttribute("data-theme", state.theme);
    state.root?.setAttribute("data-theme-mode", state.themeMode);
  }

  function appActionModuleCandidates() {
    const candidates = new Set();
    const add = (value) => {
      if (!value) return;
      try {
        const url = new URL(value, location.href);
        if (/\/assets\/rpc-[^/]+\.js$/.test(url.pathname)) candidates.add(`.${url.pathname}`);
      } catch {}
    };

    document.querySelectorAll("script[src],link[href]").forEach((node) => {
      add(node.getAttribute("src") || node.getAttribute("href"));
    });
    const resources = performance.getEntriesByType?.("resource") || [];
    resources.forEach((entry) => add(entry.name));
    return Array.from(candidates);
  }

  async function getCodexAppActions() {
    if (!codexAppActionsPromise) {
      codexAppActionsPromise = (async () => {
        const errors = [];
        for (const candidate of appActionModuleCandidates()) {
          try {
            const module = await import(candidate);
            const appActions = module?.n?.appActions || module?.appServices?.appActions;
            if (typeof appActions?.runInPrimaryWindow === "function") return appActions;
            errors.push(`${candidate}: missing appActions`);
          } catch (error) {
            errors.push(`${candidate}: ${error.message}`);
          }
        }
        throw new Error(`Codex app actions unavailable (${errors.join("; ")})`);
      })();
    }

    try {
      return await codexAppActionsPromise;
    } catch (error) {
      codexAppActionsPromise = null;
      throw error;
    }
  }

  async function setCodexThemeMode(theme) {
    if (theme !== "light" && theme !== "dark") return;
    const appActions = await getCodexAppActions();
    await appActions.runInPrimaryWindow({
      action: { type: "app.appearance.set_mode", mode: theme },
    });
  }

  function toggleCodexTheme() {
    const nextTheme = detectCodexTheme() === "dark" ? "light" : "dark";
    setCodexThemeMode(nextTheme)
      .then(() => {
        const before = `${state.themeMode}:${state.theme}`;
        syncTheme();
        if (state.open && before !== `${state.themeMode}:${state.theme}`) renderFloat();
      })
      .catch((error) => {
        console.warn("[Codex++ Stepwise] Failed to switch Codex theme", error);
      });
  }

  function themeLabel() {
    return state.theme === "dark" ? "切换到浅色主题" : "切换到深色主题";
  }

  function iconSvg(name) {
    const common = `fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"`;
    if (name === "settings") {
      return `<svg aria-hidden="true" viewBox="0 0 24 24"><path ${common} d="M12 8.8a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4Z"/><path ${common} d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.04.04a2 2 0 0 1-2.83 2.83l-.04-.04a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 0 1-4 0v-.06a1.7 1.7 0 0 0-1.03-1.56 1.7 1.7 0 0 0-1.88.34l-.04.04a2 2 0 1 1-2.83-2.83l.04-.04A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 0 1 0-4h.06A1.7 1.7 0 0 0 4.6 8.96a1.7 1.7 0 0 0-.34-1.88l-.04-.04A2 2 0 1 1 7.05 4.2l.04.04a1.7 1.7 0 0 0 1.88.34H9A1.7 1.7 0 0 0 10 3.06V3a2 2 0 0 1 4 0v.06a1.7 1.7 0 0 0 1.03 1.56h.03a1.7 1.7 0 0 0 1.88-.34l.04-.04a2 2 0 1 1 2.83 2.83l-.04.04a1.7 1.7 0 0 0-.34 1.88v.03A1.7 1.7 0 0 0 20.94 10H21a2 2 0 0 1 0 4h-.06A1.7 1.7 0 0 0 19.4 15Z"/></svg>`;
    }
    if (name === "moon") {
      return `<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="currentColor" d="M20.1 14.8A8.2 8.2 0 0 1 9.2 3.9a.9.9 0 0 0-1.1-1.1 9.8 9.8 0 1 0 13.1 13.1.9.9 0 0 0-1.1-1.1Z"/></svg>`;
    }
    if (name === "sun") {
      return `<svg aria-hidden="true" viewBox="0 0 24 24"><circle ${common} cx="12" cy="12" r="4.3"/><path ${common} d="M12 2.6v2.2M12 19.2v2.2M2.6 12h2.2M19.2 12h2.2M5.35 5.35 6.9 6.9M17.1 17.1l1.55 1.55M18.65 5.35 17.1 6.9M6.9 17.1l-1.55 1.55"/></svg>`;
    }
    if (name === "refresh") {
      return `<svg aria-hidden="true" viewBox="0 0 24 24"><path ${common} d="M20 11a8 8 0 0 0-14.1-5.2L4 8"/><path ${common} d="M4 4v4h4"/><path ${common} d="M4 13a8 8 0 0 0 14.1 5.2L20 16"/><path ${common} d="M20 20v-4h-4"/></svg>`;
    }
    return `<svg aria-hidden="true" viewBox="0 0 24 24"><path ${common} d="M6 6l12 12M18 6 6 18"/></svg>`;
  }

  function themeIcon() {
    return state.theme === "dark" ? iconSvg("sun") : iconSvg("moon");
  }

  function installThemeObserver() {
    if (state.themeObserver) return;

    let frame = 0;
    const update = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const before = `${state.themeMode}:${state.theme}`;
        syncTheme();
        if (state.open && before !== `${state.themeMode}:${state.theme}`) renderFloat();
      });
    };

    state.themeObserver = new MutationObserver(update);
    [document.documentElement, document.body].filter(Boolean).forEach((node) => {
      state.themeObserver.observe(node, {
        attributes: true,
        attributeFilter: ["class", "style", "data-theme", "color-scheme"],
      });
    });
  }

  function stripOwnUi(clone) {
    clone.querySelectorAll?.(`[${ROOT_ATTR}], [${PAYLOAD_ATTR}]`).forEach((item) => item.remove());
    return clone;
  }

  function elementText(node) {
    if (!(node instanceof Element)) return normalizeText(node?.textContent || "");
    return normalizeText(stripOwnUi(node.cloneNode(true)).textContent || "");
  }

  function directText(node) {
    if (!(node instanceof Element)) return "";
    const clone = stripOwnUi(node.cloneNode(true));
    clone.querySelectorAll?.("button,[role='button'],svg").forEach((item) => item.remove());
    return normalizeText(clone.textContent || "");
  }

  function installStyle() {
    if (document.getElementById(STYLE_ID)) return;

    const style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = `
      [${ROOT_ATTR}="true"] {
        --csw-bg: rgba(250, 250, 249, 0.98);
        --csw-border: rgba(20, 20, 20, 0.12);
        --csw-text: rgba(20, 20, 19, 0.94);
        --csw-muted: rgba(20, 20, 19, 0.62);
        --csw-soft: rgba(20, 20, 19, 0.065);
        --csw-row: rgba(255, 255, 255, 0.72);
        --csw-input: rgba(255, 255, 255, 0.82);
        --csw-fab-bg: rgba(250, 250, 249, 0.98);
        --csw-fab-fg: rgba(20, 20, 19, 0.94);
        --csw-fab-border: rgba(20, 20, 20, 0.16);
        --csw-fab-shadow: 0 10px 26px rgba(0, 0, 0, 0.14), inset 0 1px 0 rgba(255, 255, 255, 0.78);
        --csw-badge-bg: rgba(86, 86, 84, 0.98);
        --csw-badge-fg: rgba(255, 255, 255, 0.96);
        --csw-badge-border: rgba(255, 255, 255, 0.28);
        --csw-popover-shadow: 0 18px 48px rgba(0, 0, 0, 0.16);
        color: var(--csw-text);
        font: 13px/1.45 -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Hiragino Sans GB", "Segoe UI", sans-serif;
        inset: 0;
        letter-spacing: 0;
        pointer-events: none;
        position: fixed;
        z-index: 2147483000;
      }

      [${ROOT_ATTR}="true"][data-theme="dark"] {
        --csw-bg: rgba(31, 31, 30, 0.98);
        --csw-border: rgba(255, 255, 255, 0.13);
        --csw-text: rgba(247, 247, 246, 0.94);
        --csw-muted: rgba(247, 247, 246, 0.62);
        --csw-soft: rgba(255, 255, 255, 0.08);
        --csw-row: rgba(255, 255, 255, 0.06);
        --csw-input: rgba(255, 255, 255, 0.07);
        --csw-fab-bg: linear-gradient(180deg, rgba(49, 49, 48, 0.98), rgba(25, 25, 24, 0.99));
        --csw-fab-fg: rgba(255, 255, 255, 0.92);
        --csw-fab-border: rgba(255, 255, 255, 0.18);
        --csw-fab-shadow: 0 12px 30px rgba(0, 0, 0, 0.38), inset 0 1px 0 rgba(255, 255, 255, 0.08);
        --csw-badge-bg: rgba(71, 71, 69, 0.98);
        --csw-badge-fg: rgba(255, 255, 255, 0.96);
        --csw-badge-border: rgba(255, 255, 255, 0.2);
        --csw-popover-shadow: 0 18px 52px rgba(0, 0, 0, 0.38);
        color: var(--csw-text);
      }

      [${PAYLOAD_ATTR}="true"],
      [${PAYLOAD_ATTR}="block"] {
        display: none !important;
      }

      .csw-fab {
        align-items: center;
        appearance: none;
        background: var(--csw-fab-bg);
        border: 1px solid var(--csw-fab-border);
        border-radius: 999px;
        box-shadow: var(--csw-fab-shadow);
        color: var(--csw-fab-fg);
        cursor: grab;
        display: flex;
        height: 43px;
        justify-content: center;
        padding: 0;
        pointer-events: auto;
        position: fixed;
        transition: background-color 140ms ease, box-shadow 140ms ease, transform 140ms ease;
        user-select: none;
        width: 43px;
      }

      .csw-fab:active {
        cursor: grabbing;
        transform: scale(0.98);
      }

      .csw-fab:hover {
        box-shadow: var(--csw-fab-shadow), 0 0 0 4px rgba(127, 127, 127, 0.08);
      }

      .csw-fab-mark {
        align-items: center;
        display: block;
        font-size: 23px;
        font-weight: 650;
        line-height: 1;
        margin-left: 1px;
        transform: translateY(-1px);
      }

      .csw-fab-badge {
        align-items: center;
        background: var(--csw-badge-bg);
        border: 1px solid var(--csw-badge-border);
        border-radius: 999px;
        color: var(--csw-badge-fg);
        display: flex;
        font-size: 11px;
        font-weight: 700;
        height: 18px;
        justify-content: center;
        min-width: 18px;
        padding: 0 4px;
        position: absolute;
        right: -4px;
        top: -5px;
      }

      .csw-fab[data-count="0"] .csw-fab-badge {
        display: none;
      }

      .csw-popover {
        background: var(--csw-bg);
        border: 1px solid var(--csw-border);
        border-radius: 8px;
        box-shadow: var(--csw-popover-shadow);
        box-sizing: border-box;
        display: none;
        max-height: calc(100vh - 28px);
        overflow: hidden;
        pointer-events: auto;
        position: fixed;
        width: min(380px, calc(100vw - 28px));
      }

      .csw-popover[data-open="true"] {
        display: block;
      }

      .csw-head {
        align-items: center;
        border-bottom: 1px solid var(--csw-border);
        display: flex;
        gap: 8px;
        justify-content: space-between;
        padding: 9px 10px 9px 12px;
      }

      .csw-title {
        font-size: 13px;
        font-weight: 700;
      }

      .csw-tabs {
        align-items: center;
        display: flex;
        gap: 2px;
      }

      .csw-icon {
        align-items: center;
        appearance: none;
        background: transparent;
        border: 0;
        border-radius: 7px;
        color: var(--csw-muted);
        cursor: pointer;
        display: inline-flex;
        font: 600 12px/1 -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
        height: 30px;
        justify-content: center;
        padding: 0 8px;
        width: 30px;
      }

      .csw-icon[data-active="true"],
      .csw-icon:hover {
        background: var(--csw-soft);
        color: var(--csw-text);
      }

      .csw-icon:disabled {
        cursor: not-allowed;
        opacity: .42;
      }

      .csw-icon svg {
        display: block;
        height: 18px;
        width: 18px;
      }

      .csw-icon[data-action="close"] {
        font-size: 17px;
        font-weight: 500;
      }

      .csw-body {
        max-height: calc(100vh - 78px);
        overflow: auto;
        padding: 10px;
      }

      .csw-list {
        display: grid;
        gap: 6px;
      }

      .csw-row {
        appearance: none;
        background: var(--csw-row);
        border: 1px solid var(--csw-border);
        border-radius: 7px;
        color: inherit;
        cursor: pointer;
        display: block;
        min-height: 0;
        padding: 8px 9px;
        text-align: left;
        width: 100%;
      }

      .csw-row:hover,
      .csw-row:focus-visible {
        background: var(--csw-soft);
        outline: none;
      }

      .csw-row-label {
        color: var(--csw-text);
        display: block;
        font-size: 12px;
        font-weight: 700;
        margin-bottom: 3px;
      }

      .csw-row-prompt {
        color: var(--csw-muted);
        display: -webkit-box;
        font-size: 12px;
        line-height: 1.42;
        -webkit-box-orient: vertical;
        -webkit-line-clamp: 2;
        overflow: hidden;
      }

      .csw-row:hover .csw-row-prompt,
      .csw-row:focus-visible .csw-row-prompt {
        -webkit-line-clamp: 5;
      }

      .csw-empty {
        background: var(--csw-row);
        border: 1px solid var(--csw-border);
        border-radius: 7px;
        color: var(--csw-muted);
        padding: 12px;
      }

      .csw-form {
        display: grid;
        gap: 9px;
      }

      .csw-switch {
        align-items: center;
        background: var(--csw-row);
        border: 1px solid var(--csw-border);
        border-radius: 7px;
        box-sizing: border-box;
        cursor: pointer;
        display: flex;
        gap: 10px;
        justify-content: space-between;
        min-height: 40px;
        padding: 8px 9px;
      }

      .csw-switch input {
        height: 1px;
        opacity: 0;
        position: absolute;
        width: 1px;
      }

      .csw-switch-text {
        display: grid;
        gap: 2px;
      }

      .csw-switch-title {
        color: var(--csw-text);
        font-size: 12px;
        font-weight: 750;
        line-height: 1.25;
      }

      .csw-switch-note {
        color: var(--csw-muted);
        font-size: 11px;
        line-height: 1.35;
      }

      .csw-switch-control {
        background: var(--csw-soft);
        border: 1px solid var(--csw-border);
        border-radius: 999px;
        box-sizing: border-box;
        flex: 0 0 auto;
        height: 22px;
        padding: 2px;
        transition: background 140ms ease, border-color 140ms ease;
        width: 38px;
      }

      .csw-switch-control::before {
        background: var(--csw-muted);
        border-radius: 999px;
        content: "";
        display: block;
        height: 16px;
        transition: transform 140ms ease, background 140ms ease;
        width: 16px;
      }

      .csw-switch input:checked + .csw-switch-control {
        background: var(--csw-text);
        border-color: var(--csw-text);
      }

      .csw-switch input:checked + .csw-switch-control::before {
        background: var(--csw-bg);
        transform: translateX(16px);
      }

      .csw-grid {
        display: grid;
        gap: 8px;
        grid-template-columns: 1fr 1fr;
      }

      .csw-section {
        display: grid;
        gap: 8px;
      }

      .csw-section-title {
        color: var(--csw-text);
        font-size: 11px;
        font-weight: 750;
        line-height: 1.2;
      }

      .csw-summary {
        background: linear-gradient(180deg, var(--csw-row), transparent);
        border: 1px solid var(--csw-border);
        border-radius: 7px;
        display: grid;
        gap: 7px;
        padding: 9px 10px;
      }

      .csw-summary-list {
        display: grid;
      }

      .csw-summary-row {
        align-items: center;
        border-top: 1px solid var(--csw-border);
        display: grid;
        gap: 12px;
        grid-template-columns: minmax(0, 1fr) auto;
        min-height: 30px;
      }

      .csw-summary-row:first-child {
        border-top: 0;
      }

      .csw-summary-label {
        color: var(--csw-muted);
        font-size: 12px;
        font-weight: 650;
      }

      .csw-summary-value {
        color: var(--csw-text);
        font-size: 12px;
        font-weight: 700;
        max-width: 178px;
        overflow: hidden;
        text-align: right;
        text-overflow: ellipsis;
        white-space: nowrap;
      }

      .csw-summary-value[data-tone="good"],
      .csw-summary-value[data-tone="warn"],
      .csw-summary-value[data-tone="muted"] {
        border: 1px solid var(--csw-border);
        border-radius: 999px;
        padding: 2px 8px;
      }

      .csw-summary-value[data-tone="good"] {
        background: var(--csw-soft);
      }

      .csw-summary-value[data-tone="warn"] {
        color: var(--csw-muted);
      }

      .csw-summary-value[data-tone="muted"] {
        color: var(--csw-muted);
      }

      .csw-field {
        display: grid;
        gap: 4px;
      }

      .csw-field label {
        color: var(--csw-muted);
        font-size: 11px;
        font-weight: 600;
      }

      .csw-field input {
        background: var(--csw-input);
        border: 1px solid var(--csw-border);
        border-radius: 6px;
        box-sizing: border-box;
        color: var(--csw-text);
        font: inherit;
        height: 32px;
        padding: 0 8px;
        width: 100%;
      }

      .csw-field[data-disabled="true"] {
        opacity: .48;
      }

      .csw-field input:disabled {
        cursor: not-allowed;
      }

      .csw-check {
        align-items: center;
        background: var(--csw-row);
        border: 1px solid var(--csw-border);
        border-radius: 7px;
        box-sizing: border-box;
        cursor: pointer;
        display: flex;
        gap: 8px;
        min-height: 34px;
        padding: 8px 9px;
      }

      .csw-check input {
        accent-color: var(--csw-text);
        flex: 0 0 auto;
        height: 14px;
        margin: 0;
        width: 14px;
      }

      .csw-check span {
        color: var(--csw-text);
        font-size: 12px;
        font-weight: 650;
        line-height: 1.35;
      }

      .csw-actions {
        display: flex;
        gap: 7px;
        padding-top: 2px;
      }

      .csw-settings-actions {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        padding-top: 0;
      }

      .csw-primary,
      .csw-secondary {
        appearance: none;
        border-radius: 6px;
        cursor: pointer;
        font: 700 12px/1 -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
        height: 31px;
        padding: 0 11px;
      }

      .csw-primary {
        background: var(--csw-text);
        border: 0;
        color: var(--csw-bg);
      }

      .csw-secondary {
        background: transparent;
        border: 1px solid var(--csw-border);
        color: var(--csw-text);
      }

      .csw-secondary:hover,
      .csw-secondary:focus-visible {
        background: var(--csw-soft);
        outline: none;
      }

      .csw-primary:disabled,
      .csw-secondary:disabled {
        cursor: not-allowed;
        opacity: .46;
      }

      .csw-status {
        color: var(--csw-muted);
        font-size: 11px;
        min-height: 16px;
      }

      .csw-notice {
        color: var(--csw-muted);
        font-size: 11px;
        line-height: 1.4;
        min-height: 15px;
      }

    `;
    document.head.appendChild(style);
  }

  function defaultPosition() {
    return clampPosition({
      x: window.innerWidth - 76,
      y: window.innerHeight - 174,
    });
  }

  function savedPosition() {
    try {
      const parsed = JSON.parse(localStorage.getItem(POSITION_KEY) || "null");
      if (Number.isFinite(parsed?.x) && Number.isFinite(parsed?.y)) return clampPosition(parsed);
    } catch {}
    return defaultPosition();
  }

  function clampPosition(position) {
    const margin = 12;
    const size = 44;
    return {
      x: clamp(Number(position?.x) || 0, margin, Math.max(margin, window.innerWidth - size - margin)),
      y: clamp(Number(position?.y) || 0, margin, Math.max(margin, window.innerHeight - size - margin)),
    };
  }

  function savePosition(position) {
    state.position = clampPosition(position);
    localStorage.setItem(POSITION_KEY, JSON.stringify(state.position));
    applyPosition();
  }

  function applyPosition() {
    if (!state.fab || !state.position) return;
    state.position = clampPosition(state.position);
    state.fab.style.left = `${state.position.x}px`;
    state.fab.style.top = `${state.position.y}px`;
    positionPopover();
  }

  function positionPopover() {
    if (!state.popover || !state.position) return;
    const width = Math.min(380, window.innerWidth - 28);
    const measuredHeight = state.popover.offsetHeight || 260;
    const height = Math.min(measuredHeight, window.innerHeight - 28);
    const margin = 14;
    const leftSide = state.position.x > window.innerWidth / 2;
    const x = leftSide ? state.position.x - width - 12 : state.position.x + 56;
    const y = state.position.y > window.innerHeight / 2 ? state.position.y - height + 44 : state.position.y;
    state.popover.style.left = `${clamp(x, margin, Math.max(margin, window.innerWidth - width - margin))}px`;
    state.popover.style.top = `${clamp(y, margin, Math.max(margin, window.innerHeight - height - margin))}px`;
  }

  function installFloat() {
    if (!isCurrentInstance()) return;
    document.querySelectorAll?.(`[${ROOT_ATTR}="true"]`).forEach((node) => {
      if (node !== state.root) node.remove();
    });
    if (state.root && document.body.contains(state.root)) return;

    state.position = savedPosition();
    state.root = document.createElement("div");
    state.root.setAttribute(ROOT_ATTR, "true");

    state.fab = document.createElement("button");
    state.fab.className = "csw-fab";
    state.fab.type = "button";
    state.fab.title = "Stepwise";
    state.fab.innerHTML = `<span class="csw-fab-mark" aria-hidden="true">&gt;</span><span class="csw-fab-badge">0</span>`;

    state.popover = document.createElement("div");
    state.popover.className = "csw-popover";

    state.root.append(state.fab, state.popover);
    document.body.appendChild(state.root);

    state.fab.addEventListener("pointerdown", onFabPointerDown);
    state.fab.addEventListener("click", onFabClick);
    window.addEventListener("resize", onResize);
    installThemeObserver();
    syncTheme();
    applyPosition();
    renderFloat();
  }

  function onResize() {
    if (!state.position) return;
    state.position = clampPosition(state.position);
    applyPosition();
  }

  function onFabPointerDown(event) {
    if (event.button !== 0) return;
    state.drag = {
      id: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: state.position.x,
      originY: state.position.y,
      moved: false,
    };
    state.fab.setPointerCapture?.(event.pointerId);
    state.fab.addEventListener("pointermove", onFabPointerMove);
    state.fab.addEventListener("pointerup", onFabPointerUp, { once: true });
    state.fab.addEventListener("pointercancel", onFabPointerUp, { once: true });
  }

  function onFabPointerMove(event) {
    const drag = state.drag;
    if (!drag || drag.id !== event.pointerId) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    if (Math.abs(dx) + Math.abs(dy) > 4) drag.moved = true;
    if (!drag.moved) return;
    event.preventDefault();
    savePosition({ x: drag.originX + dx, y: drag.originY + dy });
  }

  function onFabPointerUp(event) {
    const drag = state.drag;
    state.fab.removeEventListener("pointermove", onFabPointerMove);
    state.fab.releasePointerCapture?.(event.pointerId);
    window.setTimeout(() => {
      if (state.drag === drag) state.drag = null;
    }, 0);
  }

  function onFabClick(event) {
    if (state.drag?.moved) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    state.open = !state.open;
    renderFloat();
  }

  function renderFloat() {
    if (!isCurrentInstance()) return;
    installStyle();
    installFloat();
    if (!state.fab || !state.popover) return;
    syncTheme();
    const count = state.prompts.length;
    state.fab.dataset.count = String(count);
    state.fab.querySelector(".csw-fab-badge").textContent = String(count);
    state.popover.dataset.open = state.open ? "true" : "false";
    if (!state.open) return;

    const refreshBlocked = state.bridgeStatus === "pending" || chatBusy();
    const refreshTitle = refreshBlocked ? "生成结束后可重新生成" : "重新生成";
    state.popover.innerHTML = `
      <div class="csw-head">
        <div class="csw-title">Stepwise</div>
        <div class="csw-tabs">
          <button class="csw-icon" type="button" data-action="refresh" title="${escapeAttr(refreshTitle)}" aria-label="${escapeAttr(refreshTitle)}" ${refreshBlocked ? "disabled" : ""}>${iconSvg("refresh")}</button>
          <button class="csw-icon" type="button" data-action="theme" title="${escapeAttr(themeLabel())}" aria-label="${escapeAttr(themeLabel())}">${themeIcon()}</button>
          <button class="csw-icon" type="button" data-action="settings-toggle" data-active="${state.activeTab === "settings"}" title="设置" aria-label="设置">${iconSvg("settings")}</button>
          <button class="csw-icon" type="button" data-action="close" aria-label="关闭">×</button>
        </div>
      </div>
      <div class="csw-body">${state.activeTab === "settings" ? settingsHtml() : nextHtml()}</div>
    `;

    state.popover.querySelector("[data-action='settings-toggle']")?.addEventListener("click", () => {
      state.activeTab = state.activeTab === "settings" ? "next" : "settings";
      if (state.activeTab === "settings") void loadSettings();
      renderFloat();
    });
    state.popover.querySelector("[data-action='close']")?.addEventListener("click", () => {
      state.open = false;
      renderFloat();
    });
    state.popover.querySelector("[data-action='refresh']")?.addEventListener("click", () => forceRefreshStepwise());
    state.popover.querySelector("[data-action='theme']")?.addEventListener("click", toggleCodexTheme);

    if (state.activeTab === "settings") attachSettingsEvents();
    else attachNextEvents();
    positionPopover();
  }

  function nextHtml() {
    if (state.bridgeStatus === "pending") {
      return `<div class="csw-empty">生成中...</div>`;
    }
    if (!state.prompts.length) {
      const text = emptyStateText();
      return `<div class="csw-empty">${escapeHtml(text)}</div>`;
    }
    return `<div class="csw-list">${state.prompts.map((item, index) => `
      <button class="csw-row" type="button" data-index="${index}">
        <span class="csw-row-label">${escapeHtml(item.label || labelForPrompt(item.prompt))}</span>
        <span class="csw-row-prompt">${escapeHtml(item.prompt)}</span>
      </button>
    `).join("")}</div>`;
  }

  function emptyStateText() {
    if (state.bridgeError) return state.bridgeError;
    if (state.bridgeStatus === "ok") return "Stepwise API 已返回，但没有解析到可用建议";
    if (state.bridgeStatus === "disabled") return "Stepwise 已关闭";
    return "当前没有可用建议";
  }

  function attachNextEvents() {
    state.popover.querySelectorAll(".csw-row").forEach((button) => {
      button.addEventListener("click", () => void selectPrompt(button));
    });
  }

  async function selectPrompt(button) {
    const item = state.prompts[Number(button.dataset.index)];
    if (!item?.prompt) return;
    if (state.settings) {
      fillSelectedPrompt(item.prompt, state.settings);
      return;
    }

    pushDiagnostic("settings:missing-before-click", {});
    const settings = await ensureSettings();
    if (!isCurrentInstance()) return;
    fillSelectedPrompt(item.prompt, settings);
  }

  function fillSelectedPrompt(prompt, settings) {
    pushDiagnostic("settings:click-mode", {
      directSend: settings?.directSend === true,
    });
    fillComposer(prompt, settings?.directSend === true);
    state.open = false;
    renderFloat();
  }

  function settingsHtml() {
    const settings = state.settings;
    if (!settings) return `<div class="csw-empty">读取中...</div>`;
    const notice = settingsNotice(settings);
    return `
      <div class="csw-form">
        <div class="csw-summary">
          <div class="csw-section-title">摘要</div>
          <div class="csw-summary-list">
            ${summaryRow("Stepwise", settings.enabled ? "已开启" : "已关闭", settings.enabled ? "good" : "muted")}
            ${summaryRow("直接发送", settings.directSend ? "已开启" : "已关闭", settings.directSend ? "good" : "muted")}
            ${summaryRow("模型", settings.model || "未配置", settings.model ? "plain" : "warn")}
            ${summaryRow("最多建议", settings.maxItems ?? 6, "plain")}
          </div>
        </div>
        <div class="csw-actions csw-settings-actions">
          <button class="csw-secondary" type="button" data-action="open-manager">设置</button>
          <button class="csw-secondary" type="button" data-action="test-settings" ${settings.enabled ? "" : "disabled"}>测试</button>
          <button class="csw-secondary" type="button" data-action="reset-position">归位</button>
        </div>
        ${notice ? `<div class="csw-notice">${escapeHtml(notice)}</div>` : ""}
      </div>
    `;
  }

  function summaryRow(label, value, tone = "plain") {
    return `
      <div class="csw-summary-row">
        <span class="csw-summary-label">${escapeHtml(label)}</span>
        <span class="csw-summary-value" data-tone="${escapeAttr(tone)}">${escapeHtml(value)}</span>
      </div>
    `;
  }

  function settingsNotice(settings) {
    const status = state.settingsStatus || "";
    const line = statusLine(settings);
    if (!status || status === line) {
      if (settings.enabled && settings.baseUrlConfigured && settings.model && settings.apiKeyConfigured) return "";
      return line;
    }
    return status;
  }

  function statusLine(settings) {
    if (settings.enabled !== true) return "Stepwise 已关闭，请在 Codex++ Manager 里开启。";
    if (!settings.baseUrlConfigured || !settings.model) return "Stepwise 已开启，但 Base URL 或 Model 未配置。";
    if (!settings.apiKeyConfigured) return `Stepwise 已开启，但 API Key 未配置；可填写密钥或设置 ${settings.apiKeyEnv || "环境变量"}。`;
    return `Stepwise 已开启 · ${settings.model || ""}`.replace(/\s+·\s+$/, "");
  }

  function attachSettingsEvents() {
    state.popover.querySelector("[data-action='open-manager']")?.addEventListener("click", () => void openManager());
    state.popover.querySelector("[data-action='test-settings']")?.addEventListener("click", () => void testSettings());
    state.popover.querySelector("[data-action='reset-position']")?.addEventListener("click", () => {
      localStorage.removeItem(POSITION_KEY);
      state.position = defaultPosition();
      applyPosition();
      state.settingsStatus = "位置已归位";
      renderFloat();
    });
  }

  async function loadSettings() {
    const payload = await bridgeCall("/stepwise/settings", {});
    if (!isCurrentInstance()) return null;
    if (payload?.settings) {
      state.settings = payload.settings;
      state.settingsStatus = statusLine(payload.settings);
    } else {
      state.settingsStatus = payload?.error || "Bridge 未就绪";
    }
    if (state.activeTab === "settings" && state.open) renderFloat();
    return state.settings;
  }

  async function ensureSettings() {
    if (state.settings) return state.settings;
    if (!settingsPromise) {
      settingsPromise = loadSettings().finally(() => {
        settingsPromise = null;
      });
    }
    return settingsPromise;
  }

  async function testSettings() {
    state.settingsStatus = "测试中...";
    renderFloat();
    const payload = await bridgeCall("/stepwise/test", {});
    if (!isCurrentInstance()) return;
    const count = Array.isArray(payload?.items) ? payload.items.length : 0;
    state.settingsStatus = payload?.error || (payload?.disabled ? "已关闭" : `测试通过 · ${count} 条`);
    renderFloat();
  }

  async function openManager() {
    state.settingsStatus = "正在打开 Codex++ Manager...";
    renderFloat();
    const payload = await bridgeCall("/manager/open", {});
    if (!isCurrentInstance()) return;
    state.settingsStatus = payload?.status === "ok" ? "已打开 Manager" : payload?.message || "打开失败";
    renderFloat();
  }

  function bridgeCall(path, payload) {
    if (typeof window[PAGE_BRIDGE] !== "function") {
      return Promise.resolve({ error: "page bridge is not installed", items: [] });
    }
    let timer = 0;
    const timeout = new Promise((resolve) => {
      timer = window.setTimeout(() => resolve({ error: "page bridge timed out", items: [] }), BRIDGE_TIMEOUT_MS);
    });
    const request = Promise.resolve(window[PAGE_BRIDGE](path, payload || {}));
    return Promise.race([request, timeout]).finally(() => window.clearTimeout(timer));
  }

  function roleFromElement(node) {
    if (!(node instanceof Element)) return "";
    const explicit = node.getAttribute("data-message-author-role");
    if (explicit) return explicit.toLowerCase();

    const text = elementText(node);
    if (/^(assistant|codex|assistant\s+said)\b/i.test(text)) return "assistant";
    if (/^(user|you)\b/i.test(text)) return "user";
    return "";
  }

  function chatRoot() {
    return Array.from(document.querySelectorAll(".thread-scroll-container"))
      .filter((node) => visibleElement(node) && !state.root?.contains(node))
      .sort((left, right) => {
        const leftRect = visibleRect(left);
        const rightRect = visibleRect(right);
        return (rightRect.width * rightRect.height) - (leftRect.width * leftRect.height);
      })[0] || null;
  }

  function elementCenter(rect) {
    if (!rect) return { x: 0, y: 0 };
    return {
      x: rect.left + rect.width / 2,
      y: rect.top + rect.height / 2,
    };
  }

  function horizontalOverlapRatio(left, right) {
    if (!left || !right) return 0;
    const overlap = Math.max(0, Math.min(left.right, right.right) - Math.max(left.left, right.left));
    return overlap / Math.max(1, Math.min(left.width, right.width));
  }

  function ignoredComposerContainer(node) {
    if (!(node instanceof Element)) return true;
    if (state.root?.contains(node)) return true;
    return Boolean(node.closest([
      `[${ROOT_ATTR}="true"]`,
      `[${PAYLOAD_ATTR}="true"]`,
      "aside",
      "nav",
      "[role='dialog']",
      "[aria-modal='true']",
      "[role='menu']",
      "[role='listbox']",
    ].join(",")));
  }

  function composerCandidateScore(node, rootRect) {
    const rect = visibleRect(node);
    if (!rect || !rootRect) return -Infinity;
    if (rect.width < 120 || rect.height < 20) return -Infinity;
    if (rect.bottom < window.innerHeight * 0.35) return -Infinity;
    if (ignoredComposerContainer(node)) return -Infinity;

    const overlap = horizontalOverlapRatio(rect, rootRect);
    const center = elementCenter(rect);
    const rootCenter = elementCenter(rootRect);
    const centerDrift = Math.abs(center.x - rootCenter.x) / Math.max(1, rootRect.width);
    const centerInsideRoot = center.x >= rootRect.left - 24 && center.x <= rootRect.right + 24;
    if (overlap < 0.45 && !centerInsideRoot) return -Infinity;

    const lowerScreen = rect.bottom / Math.max(1, window.innerHeight);
    const widthMatch = Math.min(rect.width, rootRect.width) / Math.max(1, Math.max(rect.width, rootRect.width));
    return overlap * 100 + lowerScreen * 24 + widthMatch * 18 - centerDrift * 48;
  }

  function mainComposerCandidate(candidates) {
    const rootRect = visibleRect(chatRoot());
    const ranked = candidates
      .map((node) => ({ node, score: composerCandidateScore(node, rootRect) }))
      .filter((item) => Number.isFinite(item.score))
      .sort((left, right) => right.score - left.score);
    return ranked[0]?.node || null;
  }

  function composerCandidates() {
    return Array.from(
      document.querySelectorAll(
        [
          "textarea",
          "[contenteditable='true']",
          "[role='textbox']",
          "div.ProseMirror",
        ].join(",")
      )
    ).filter((node) => {
      if (!(node instanceof HTMLElement)) return false;
      const rect = node.getBoundingClientRect();
      if (rect.width < 120 || rect.height < 20) return false;
      if (rect.bottom < window.innerHeight * 0.35) return false;
      if (ignoredComposerContainer(node)) return false;
      return true;
    });
  }

  function buttonLabel(node) {
    return normalizeText(node.getAttribute("aria-label") || node.getAttribute("title") || node.textContent || "");
  }

  function sendButtonLabel(label) {
    return /^(send message|send|发送消息|发送|提交)$/i.test(label);
  }

  function stopButtonLabel(label) {
    return /^(stop|停止)$/i.test(label);
  }

  function iconPathData(node) {
    return Array.from(node.querySelectorAll?.("svg path") || [])
      .map((path) => path.getAttribute("d") || "")
      .join("\n");
  }

  function stopButtonIcon(node) {
    const data = iconPathData(node);
    return /H14\.25C14\.9404 4\.5 15\.5 5\.05964 15\.5 5\.75V14\.25C15\.5 14\.9404/.test(data);
  }

  function stopButton(node) {
    return stopButtonLabel(buttonLabel(node)) || stopButtonIcon(node);
  }

  function disabledButton(node) {
    return Boolean(node.disabled || node.getAttribute("aria-disabled") === "true" || node.dataset.disabled === "true");
  }

  function submitButtonCandidate(button, containerRect) {
    const label = buttonLabel(button);
    if (stopButton(button)) return false;
    if (sendButtonLabel(label)) return true;
    if (label) return false;

    const rect = visibleRect(button);
    if (!rect || !containerRect) return false;
    const className = String(button.className || "");
    const compactIcon = rect.width >= 24 && rect.width <= 48 && rect.height >= 24 && rect.height <= 48;
    const composerIcon = className.includes("size-token-button-composer") || className.includes("bg-token-foreground");
    const lowerRight = rect.left > containerRect.left + containerRect.width * 0.58 &&
      rect.top > containerRect.top + containerRect.height * 0.42;
    return compactIcon && composerIcon && lowerRight;
  }

  function nearbySubmitButton(target, options = {}) {
    const includeDisabled = options.includeDisabled === true;
    let current = target?.parentElement || null;
    for (let depth = 0; current && depth < 8; depth += 1, current = current.parentElement) {
      if (current === document.body || current === document.documentElement) break;
      if (state.root?.contains(current)) return null;
      const buttons = Array.from(current.querySelectorAll("button,[role='button']"))
        .filter((node) => node instanceof HTMLElement && !state.root?.contains(node) && visibleElement(node) && (includeDisabled || !disabledButton(node)));

      const labeled = buttons.find((button) => sendButtonLabel(buttonLabel(button)));
      if (labeled) return labeled;

      const rect = visibleRect(current);
      if (rect && rect.width > 260 && rect.height > 52) {
        const lowerRight = buttons
          .filter((button) => !stopButton(button))
          .filter((button) => submitButtonCandidate(button, rect))
          .sort((a, b) => b.getBoundingClientRect().right - a.getBoundingClientRect().right);
        if (lowerRight.length) return lowerRight[0];
      }
    }
    return null;
  }

  function chatSurfaceReady() {
    if (!chatRoot()) return false;
    return !chatBusy();
  }

  function chatBusy() {
    const root = chatRoot();
    if (!root) return false;

    return Array.from(root.querySelectorAll("button,[role='button']")).some((node) => {
      if (!visibleElement(node)) return false;
      const label = normalizeText(node.getAttribute("aria-label") || node.textContent || "");
      return /^(停止|stop)$/i.test(label);
    });
  }

  function setScanStatus(status, details = {}) {
    const key = `${status}:${JSON.stringify(details)}`;
    if (state.lastScanStatus === key) return;
    state.lastScanStatus = key;
    pushDiagnostic(`scan:${status}`, details);
  }

  function composerBusy(target) {
    let current = target?.parentElement || null;
    for (let depth = 0; current && depth < 8; depth += 1, current = current.parentElement) {
      if (current === document.body || current === document.documentElement) break;
      if (state.root?.contains(current)) return false;
      const buttons = Array.from(current.querySelectorAll("button,[role='button']"));
      if (buttons.some((node) => {
        if (!visibleElement(node)) return false;
        return stopButton(node);
      })) return true;
    }
    return false;
  }

  function messageCandidates() {
    const root = chatRoot();
    if (!root) return [];

    const selectors = [
      "[data-message-author-role]",
      "[data-thread-find-target]",
      "[data-testid*='message' i]",
      "[data-test-id*='message' i]",
      "article",
    ].join(",");

    return Array.from(root.querySelectorAll(selectors))
      .filter(visibleElement)
      .map((node) => ({
        node,
        role: roleFromElement(node),
        text: elementText(node),
      }))
      .filter((item) => item.text.length > 8);
  }

  function actionButton(node) {
    const label = normalizeText(node.getAttribute("aria-label") || node.textContent || "");
    return /^(复制|喜欢|不喜欢|从此处开始分叉|挂钩|copy|like|dislike|fork)/i.test(label);
  }

  function classTokenMatch(node, token) {
    return node instanceof Element && Array.from(node.classList || []).some((className) => className === token);
  }

  function assistantBubbleCandidates() {
    const root = chatRoot();
    if (!root) return [];

    return Array.from(root.querySelectorAll(".group.flex.min-w-0.flex-col"))
      .filter((node) => {
        if (!(node instanceof HTMLElement)) return false;
        if (state.root?.contains(node)) return false;
        if (classTokenMatch(node, "items-end")) return false;
        const text = directText(node);
        if (text.length < 24 || text.length > MAX_TEXT_LENGTH) return false;
        return true;
      })
      .map((node) => ({
        node,
        role: "assistant",
        text: elementText(node),
      }));
  }

  function latestMessageByDocumentOrder(candidates) {
    return candidates
      .filter((item) => item?.node instanceof Node && item.text?.length > 8)
      .sort((left, right) => {
        if (left.node === right.node) return 0;
        const position = left.node.compareDocumentPosition(right.node);
        if (position & Node.DOCUMENT_POSITION_FOLLOWING) return -1;
        if (position & Node.DOCUMENT_POSITION_PRECEDING) return 1;
        if (left.node.contains(right.node)) return -1;
        if (right.node.contains(left.node)) return 1;
        return 0;
      })
      .at(-1) || null;
  }

  function actionRowForMessage(root) {
    const buttons = Array.from(root.querySelectorAll("button,[role='button']")).filter(actionButton);
    for (const button of buttons) {
      let current = button.parentElement;
      for (let depth = 0; current && depth < 5; depth += 1, current = current.parentElement) {
        const rect = visibleRect(current);
        if (!rect || rect.height > 96) continue;
        const count = Array.from(current.querySelectorAll("button,[role='button']")).filter(actionButton).length;
        if (count >= 2) return current;
      }
    }
    return null;
  }

  function containsActionRow(node) {
    return Boolean(node && actionRowForMessage(node));
  }

  function assistantContainerForActionRow(actionRow) {
    let current = actionRow?.parentElement;

    for (let depth = 0; current && depth < 7; depth += 1, current = current.parentElement) {
      const text = directText(current);
      if (text.length < 24) continue;
      if (text.length > MAX_TEXT_LENGTH) continue;
      if (!containsActionRow(current)) continue;
      return current;
    }

    return null;
  }

  function allActionRows() {
    const root = chatRoot();
    if (!root) return [];

    const rows = [];
    const seen = new Set();
    const buttons = Array.from(root.querySelectorAll("button,[role='button']")).filter(actionButton);

    for (const button of buttons) {
      let current = button.parentElement;
      for (let depth = 0; current && depth < 5; depth += 1, current = current.parentElement) {
        if (seen.has(current)) continue;
        if (!visibleElement(current)) continue;
        const rect = visibleRect(current);
        if (!rect || rect.height > 96) continue;
        const count = Array.from(current.querySelectorAll("button,[role='button']")).filter(actionButton).length;
        if (count < 2) continue;
        seen.add(current);
        rows.push(current);
        break;
      }
    }

    return rows;
  }

  function findLatestAssistantMessage() {
    const candidates = [];
    const rows = allActionRows();
    for (let index = 0; index < rows.length; index += 1) {
      const node = assistantContainerForActionRow(rows[index]);
      const text = elementText(node);
      if (text.length > 8) candidates.push({ node, role: "assistant", text });
    }

    candidates.push(...messageCandidates().filter((item) => item.role === "assistant"));
    candidates.push(...assistantBubbleCandidates());
    return latestMessageByDocumentOrder(candidates);
  }

  function findPreviousUserText(assistantNode) {
    const candidates = messageCandidates();
    const before = candidates.filter((item) => {
      if (item.node === assistantNode) return false;
      if (!(item.node instanceof Node) || !(assistantNode instanceof Node)) return false;
      return Boolean(item.node.compareDocumentPosition(assistantNode) & Node.DOCUMENT_POSITION_FOLLOWING);
    });

    for (let cursor = before.length - 1; cursor >= 0; cursor -= 1) {
      const item = before[cursor];
      if (item.role === "user") return shortText(item.text, 2000);
      if (/^(user|you)\b/i.test(item.text)) return shortText(item.text, 2000);
    }
    return "";
  }

  function hideStepwisePayload(root) {
    if (!(root instanceof Element)) return;

    const blocks = Array.from(root.querySelectorAll("pre, code")).filter((node) => {
      if (!(node instanceof Element)) return false;
      return /"codex_stepwise"\s*:\s*true/.test(node.textContent || "");
    });

    for (const block of blocks) {
      const container = block.closest("[class*='_codeBlock_'], pre") || block;
      container.setAttribute(PAYLOAD_ATTR, "true");
    }
  }

  function uniquePrompts(items) {
    const seen = new Set();
    const result = [];
    for (const item of items) {
      const prompt = normalizeText(typeof item === "string" ? item : item.prompt).replace(/\s+/g, " ");
      if (!prompt || seen.has(prompt)) continue;
      seen.add(prompt);
      result.push({
        label: normalizeText(typeof item === "string" ? labelForPrompt(prompt) : item.label || labelForPrompt(prompt)),
        prompt,
      });
      if (result.length >= MAX_STEPWISE_ITEMS) break;
    }
    return result;
  }

  function labelForPrompt(prompt) {
    const text = normalizeText(prompt);
    const rules = [
      [/diff|风险分级|改动.*总结/i, "查看 diff"],
      [/commit|提交/i, "整理 commit"],
      [/截图验证|遮挡|浮球|面板/i, "验证界面"],
      [/设置|配置|Bridge|API/i, "检查配置"],
      [/Codex\+\+|用户脚本|reload|生效/i, "检查脚本"],
      [/只读验证|确认.*生效|验证步骤/i, "验证生效"],
      [/错误|失败|最小复现|排查/i, "继续排查"],
      [/P0|P1|P2|执行顺序/i, "分级排序"],
      [/维护成本|长期稳定性|审查/i, "重新审查"],
      [/文件路径|当前状态|继续追踪/i, "列出路径"],
      [/下一步|改哪些文件/i, "继续下一步"],
      [/遗漏的风险|回滚方式/i, "风险回滚"],
    ];

    for (const [pattern, label] of rules) {
      if (pattern.test(text)) return label;
    }

    return text
      .replace(/^(帮我|请|把|给我|继续|检查|执行一次|基于刚才的)/, "")
      .replace(/[，。,.].*$/, "")
      .trim()
      .slice(0, 10) || "继续";
  }

  function parseStepwiseJson(text) {
    const blocks = Array.from(text.matchAll(/```(?:json)?\s*([\s\S]*?)```/gi))
      .map((match) => match[1])
      .filter((block) => /"codex_stepwise"\s*:\s*true/.test(block));

    for (const block of blocks.reverse()) {
      const parsed = parsePayloadCandidate(block);
      if (parsed) return parsed;
    }
    return parsePayloadCandidate(extractJsonObject(text));
  }

  function parsePayloadCandidate(value) {
    const text = normalizeText(value)
      .replace(/^```(?:json)?/i, "")
      .replace(/```$/i, "")
      .replace(/^json\s+/i, "")
      .trim();

    if (!/"codex_stepwise"\s*:\s*true/.test(text)) return null;

    try {
      const parsed = JSON.parse(text);
      return parsed && parsed.codex_stepwise === true ? parsed : null;
    } catch {
      return null;
    }
  }

  function extractJsonObject(text) {
    const source = String(text || "");
    const marker = source.search(/"codex_stepwise"\s*:\s*true/);
    if (marker < 0) return "";

    const start = source.lastIndexOf("{", marker);
    if (start < 0) return "";

    let depth = 0;
    let inString = false;
    let escaped = false;

    for (let index = start; index < source.length; index += 1) {
      const char = source[index];
      if (escaped) {
        escaped = false;
        continue;
      }
      if (char === "\\") {
        escaped = true;
        continue;
      }
      if (char === "\"") {
        inString = !inString;
        continue;
      }
      if (inString) continue;
      if (char === "{") depth += 1;
      if (char === "}") depth -= 1;
      if (depth === 0) return source.slice(start, index + 1);
    }

    return "";
  }

  function stripStepwisePayloadText(text) {
    const withoutFence = String(text || "").replace(/```(?:json)?\s*[\s\S]*?"codex_stepwise"\s*:\s*true[\s\S]*?```/gi, "");
    const payloadObject = extractJsonObject(withoutFence);
    return normalizeText(payloadObject ? withoutFence.replace(payloadObject, "") : withoutFence);
  }

  function payloadFromDom(root) {
    if (!(root instanceof Element)) return null;
    const blocks = Array.from(root.querySelectorAll("pre, code"))
      .filter((node) => /"codex_stepwise"\s*:\s*true/.test(node.textContent || ""));

    for (const block of blocks.reverse()) {
      const parsed = parsePayloadCandidate(block.textContent || "");
      if (parsed) return parsed;
    }

    return null;
  }

  function payloadItems(payload) {
    if (!payload) return [];
    if (Array.isArray(payload)) return payload;
    for (const key of ["items", "suggestions", "next_steps", "nextSteps", "actions", "prompts"]) {
      if (Array.isArray(payload[key])) return payload[key];
    }
    return [];
  }

  function payloadPrompts(payload) {
    const rawItems = payloadItems(payload);
    if (!rawItems.length) return [];
    const items = rawItems
      .slice(0, MAX_STEPWISE_ITEMS)
      .map((item) => {
        const prompt = shortText(
          typeof item === "string"
            ? item
            : item?.prompt || item?.text || item?.action || item?.content || item?.message || "",
          MAX_PROMPT_LENGTH
        ).replace(/\s+/g, " ");
        const label = shortText(
          typeof item === "string" ? "" : item?.label || item?.title || item?.name || "",
          36
        ).replace(/\s+/g, " ");
        return prompt ? { label: label || labelForPrompt(prompt), prompt } : null;
      })
      .filter(Boolean);
    return uniquePrompts(items);
  }

  function extractStepwisePayload(message) {
    const text = elementText(message.node);
    const payload = payloadFromDom(message.node) || parseStepwiseJson(text);
    return {
      payload,
      prompts: payloadPrompts(payload),
      textWithoutPayload: stripStepwisePayloadText(text),
    };
  }

  function bridgeRequestKey(userText, assistantText) {
    return hashText(`${shortText(userText, 2400)}\n\n--- assistant ---\n\n${shortText(assistantText, 5200)}`);
  }

  function requestBridgeStepwise(key, userText, assistantText) {
    if (!key || state.bridgePendingHash === key || state.bridgeCache.has(key)) return;

    state.bridgePendingHash = key;
    state.bridgeStatus = "pending";
    state.bridgeError = "";
    renderFloat();

    bridgeCall(
      "/stepwise/generate",
      {
        request: {
        lastUserMessage: userText,
        lastAssistantMessage: assistantText,
        threadTitle: document.title || "",
        pageUrl: location.href,
      },
      }
    )
      .then((payload) => {
        if (!isCurrentInstance()) return;
        const prompts = payload?.disabled || payload?.error ? [] : payloadPrompts(payload);
        pushDiagnostic("bridge:generate-result", {
          status: payload?.status || "",
          disabled: Boolean(payload?.disabled),
          error: normalizeText(payload?.error || ""),
          rawItemCount: payloadItems(payload).length,
          promptCount: prompts.length,
          payloadKeys: payload && typeof payload === "object" ? Object.keys(payload).slice(0, 12) : [],
        });
        state.bridgeCache.set(key, {
          disabled: Boolean(payload?.disabled),
          error: normalizeText(payload?.error || ""),
          prompts,
        });
        state.bridgeStatus = payload?.disabled ? "disabled" : payload?.error ? "failed" : "ok";
        state.bridgeError = normalizeText(payload?.error || "");
      })
      .catch((error) => {
        if (!isCurrentInstance()) return;
        pushDiagnostic("bridge:generate-failed", { error: error.message });
        state.bridgeCache.set(key, { disabled: true, error: error.message, prompts: [] });
        state.bridgeStatus = "failed";
        state.bridgeError = error.message;
      })
      .finally(() => {
        if (!isCurrentInstance()) return;
        if (state.bridgePendingHash === key) state.bridgePendingHash = "";
        scheduleScan(0);
      });
  }

  function forceRefreshStepwise() {
    if (!isCurrentInstance()) return;
    if (state.bridgeStatus === "pending") {
      setScanStatus("manual-refresh-pending", {});
      return;
    }
    if (chatBusy()) {
      if (!state.prompts.length) state.bridgeError = "回答生成中，结束后再刷新";
      setScanStatus("manual-refresh-busy", {});
      renderFloat();
      return;
    }

    const message = findLatestAssistantMessage();
    if (!message) {
      state.bridgeError = "未找到可用于生成的回答";
      state.prompts = [];
      setScanStatus("manual-refresh-no-assistant", {});
      renderFloat();
      return;
    }

    const stepwisePayload = extractStepwisePayload(message);
    hideStepwisePayload(message.node);
    const assistantText = shortText(stepwisePayload.textWithoutPayload || message.text);
    const userText = findPreviousUserText(message.node);
    const bridgeKey = bridgeRequestKey(userText, assistantText);
    if (bridgeKey) state.bridgeCache.delete(bridgeKey);

    state.lastAssistantHash = hashText(assistantText);
    state.lastAssistantAt = 0;
    state.currentHash = `${state.lastAssistantHash}:manual-refresh`;
    state.prompts = [];
    state.bridgeError = "";
    setScanStatus("manual-refresh", { hash: state.lastAssistantHash, textLength: assistantText.length });
    requestBridgeStepwise(bridgeKey, userText, assistantText);
    renderFloat();
  }

  function clearPromptsForNewAssistant(hash) {
    state.currentHash = `${hash}:pending`;
    state.prompts = [];
    state.bridgeError = "";
    renderFloat();
  }

  function setNativeValue(element, value) {
    const prototype = Object.getPrototypeOf(element);
    const descriptor = Object.getOwnPropertyDescriptor(prototype, "value");
    if (descriptor && typeof descriptor.set === "function") descriptor.set.call(element, value);
    else element.value = value;
  }

  function composerText(target) {
    if (target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement) return normalizeText(target.value);
    return normalizeText(target?.textContent || "");
  }

  function pressEnter(target) {
    target.focus();
    const base = {
      key: "Enter",
      code: "Enter",
      keyCode: 13,
      which: 13,
      bubbles: true,
      cancelable: true,
      composed: true,
    };
    const down = target.dispatchEvent(new KeyboardEvent("keydown", base));
    target.dispatchEvent(new KeyboardEvent("keypress", base));
    target.dispatchEvent(new KeyboardEvent("keyup", base));
    pushDiagnostic("submit:enter-fallback", { defaultAllowed: down });
    return true;
  }

  function submitComposer(target, allowFallback = false) {
    if (!(target instanceof HTMLElement)) return false;
    if (composerBusy(target)) {
      pushDiagnostic("submit:blocked-local-stop", { attemptFallback: allowFallback });
      return false;
    }

    const button = nearbySubmitButton(target);
    if (button) {
      pushDiagnostic("submit:button-click", {
        label: buttonLabel(button),
        disabled: disabledButton(button),
        rect: rectSummary(button),
        className: String(button.className || "").slice(0, 160),
        composerTextLength: composerText(target).length,
        iconPath: iconPathData(button).slice(0, 160),
      });
      button.click();
      return true;
    }

    const pendingButton = nearbySubmitButton(target, { includeDisabled: true });
    if (pendingButton && disabledButton(pendingButton)) {
      pushDiagnostic("submit:button-disabled", {
        label: buttonLabel(pendingButton),
        rect: rectSummary(pendingButton),
        className: String(pendingButton.className || "").slice(0, 160),
        composerTextLength: composerText(target).length,
        iconPath: iconPathData(pendingButton).slice(0, 160),
      });
      return false;
    }

    const form = target.closest("form");
    if (form && allowFallback) {
      pushDiagnostic("submit:form-fallback", { rect: rectSummary(form) });
      try {
        form.requestSubmit();
      } catch {
        pushDiagnostic("submit:form-fallback-failed", {});
        return false;
      }
      return true;
    }

    if (allowFallback) return pressEnter(target);
    pushDiagnostic("submit:no-button-yet", { allowFallback });
    return false;
  }

  function submitComposerWhenReady(target, expectedText = "", attempt = 0) {
    if (!(target instanceof HTMLElement)) return false;
    if (!document.contains(target)) {
      pushDiagnostic("submit:target-detached", { attempt });
      return false;
    }
    if (normalizeText(expectedText) && composerText(target) !== normalizeText(expectedText)) {
      pushDiagnostic("submit:composer-changed", {
        attempt,
        expectedLength: normalizeText(expectedText).length,
        actualLength: composerText(target).length,
      });
      return false;
    }
    if (composerBusy(target)) {
      if (attempt === 0 || attempt % 10 === 0 || attempt >= SUBMIT_RETRY_LIMIT) {
        pushDiagnostic("submit:blocked-local-stop", {
          attempt,
          retrying: attempt < SUBMIT_RETRY_LIMIT,
          targetRect: rectSummary(target),
        });
      }
      if (attempt >= SUBMIT_RETRY_LIMIT) {
        pushDiagnostic("submit:blocked-local-stop-timeout", { attempt, targetRect: rectSummary(target) });
        return false;
      }
      window.setTimeout(() => submitComposerWhenReady(target, expectedText, attempt + 1), SUBMIT_RETRY_DELAY_MS);
      return false;
    }
    if (submitComposer(target, attempt >= SUBMIT_RETRY_LIMIT)) return true;
    if (attempt >= SUBMIT_RETRY_LIMIT) return false;
    window.setTimeout(() => submitComposerWhenReady(target, expectedText, attempt + 1), SUBMIT_RETRY_DELAY_MS);
    return false;
  }

  function setEditableText(target, prompt) {
    target.focus();
    const selection = window.getSelection?.();
    const range = document.createRange();
    range.selectNodeContents(target);
    selection?.removeAllRanges();
    selection?.addRange(range);

    let inserted = false;
    try {
      inserted = document.execCommand?.("insertText", false, prompt) === true;
    } catch {
      inserted = false;
    }
    if (!inserted) target.textContent = prompt;
  }

  function fillComposer(prompt, submit = false) {
    const candidates = composerCandidates();
    const target = mainComposerCandidate(candidates);
    pushDiagnostic("fill:start", {
      submit,
      candidateCount: candidates.length,
      targetTag: target?.tagName || "",
      targetRole: target?.getAttribute?.("role") || "",
      targetClass: String(target?.className || "").slice(0, 120),
      targetRect: rectSummary(target),
      chatRootRect: rectSummary(chatRoot()),
      promptLength: normalizeText(prompt).length,
    });
    if (!target) {
      pushDiagnostic("fill:no-main-composer", { candidateCount: candidates.length });
      window.prompt("Copy Stepwise prompt", prompt);
      return false;
    }

    target.focus();
    if (target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement) {
      setNativeValue(target, prompt);
      target.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: prompt }));
      target.dispatchEvent(new Event("change", { bubbles: true }));
      pushDiagnostic("fill:text-control", { valueLength: normalizeText(target.value).length });
      if (submit) submitComposerWhenReady(target, prompt);
      return true;
    }

    if (target.isContentEditable || target.getAttribute("role") === "textbox") {
      setEditableText(target, prompt);
      target.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: prompt }));
      pushDiagnostic("fill:editable", { valueLength: normalizeText(target.textContent).length });
      if (submit) window.setTimeout(() => submitComposerWhenReady(target, prompt), EDITABLE_SUBMIT_DELAY_MS);
      return true;
    }

    window.prompt("Copy Stepwise prompt", prompt);
    return false;
  }

  function scan() {
    if (!isCurrentInstance()) return;
    state.timer = 0;
    state.scans += 1;
    installStyle();
    installFloat();

    if (!chatSurfaceReady()) {
      setScanStatus("not-ready", {
        hasRoot: Boolean(chatRoot()),
        composerCount: composerCandidates().length,
        busy: chatBusy(),
      });
      renderFloat();
      return;
    }

    const message = findLatestAssistantMessage();
    if (!message) {
      setScanStatus("no-assistant-message", {
        messageCandidateCount: messageCandidates().length,
        actionRowCount: allActionRows().length,
      });
      renderFloat();
      return;
    }

    const stepwisePayload = extractStepwisePayload(message);
    hideStepwisePayload(message.node);

    const assistantText = shortText(stepwisePayload.textWithoutPayload || message.text);
    const hash = hashText(assistantText);
    const now = Date.now();

    if (hash !== state.lastAssistantHash) {
      state.lastAssistantHash = hash;
      state.lastAssistantAt = now;
      if (state.prompts.length || state.currentHash) clearPromptsForNewAssistant(hash);
      setScanStatus("assistant-changed", { hash, textLength: assistantText.length });
      scheduleScan(STREAM_IDLE_MS + 120);
      return;
    }

    if (now - state.lastAssistantAt < STREAM_IDLE_MS) {
      setScanStatus("assistant-settling", { hash });
      scheduleScan(STREAM_IDLE_MS);
      return;
    }

    const userText = findPreviousUserText(message.node);
    const bridgeKey = bridgeRequestKey(userText, assistantText);
    const bridgeResult = state.bridgeCache.get(bridgeKey);
    const prompts = bridgeResult?.prompts?.length ? bridgeResult.prompts : stepwisePayload.prompts;

    if (!bridgeResult) {
      pushDiagnostic("bridge:generate-request", {
        userTextLength: userText.length,
        assistantTextLength: assistantText.length,
        hasInlinePayload: Boolean(stepwisePayload.payload),
        inlinePromptCount: stepwisePayload.prompts.length,
      });
      requestBridgeStepwise(bridgeKey, userText, assistantText);
    }
    setScanStatus("ready", {
      hash,
      bridgeCached: Boolean(bridgeResult),
      promptCount: prompts.length,
    });

    const nextHash = hashText(prompts.map((item) => `${item.label}\n${item.prompt}`).join("\n\n"));
    if (state.currentHash !== `${hash}:${nextHash}`) {
      state.currentHash = `${hash}:${nextHash}`;
      state.prompts = prompts;
      renderFloat();
    }
  }

  function scheduleScan(delay = SCAN_DELAY_MS) {
    if (!isCurrentInstance()) return;
    if (state.timer) window.clearTimeout(state.timer);
    state.timer = window.setTimeout(scan, delay);
  }

  function installObserver() {
    if (!isCurrentInstance()) return false;
    const root = document.body || document.documentElement;
    if (!root) return false;

    state.observer = new MutationObserver((mutations) => {
      const relevant = mutations.some((mutation) => {
        if (state.root?.contains(mutation.target)) return false;
        return mutation.addedNodes.length || mutation.type === "characterData";
      });
      if (relevant) scheduleScan();
    });
    state.observer.observe(root, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    return true;
  }

  function stopRuntime() {
    if (state.timer) window.clearTimeout(state.timer);
    state.timer = 0;
    window.removeEventListener("resize", onResize);
    state.observer?.disconnect();
    state.observer = null;
    state.themeObserver?.disconnect();
    state.themeObserver = null;
    state.root?.remove();
    state.root = null;
    state.fab = null;
    state.popover = null;
    document.getElementById(STYLE_ID)?.remove();
    state.open = false;
  }

  function activateRuntime() {
    installStyle();
    installFloat();
    if (!state.observer && !installObserver()) {
      document.addEventListener(
        "DOMContentLoaded",
        () => {
          if (!isCurrentInstance()) return;
          installObserver();
          installFloat();
          void ensureSettings();
          scheduleScan(0);
        },
        { once: true }
      );
    }
    scheduleScan(0);
  }

  async function syncSettings(patch = {}) {
    if (!isCurrentInstance()) return null;
    if (patch && typeof patch === "object") {
      state.settings = { ...(state.settings || {}), ...patch };
    }
    if (patch?.enabled === false) {
      stopRuntime();
      settingsPromise = null;
      startupPromise = null;
      const settings = await loadSettings();
      if (!isCurrentInstance()) return null;
      if (settings?.enabled) activateRuntime();
      else pushDiagnostic("settings:disabled-sync", {});
      return settings;
    }
    if (patch?.enabled === true) {
      pushDiagnostic("settings:enabled-sync", {});
      activateRuntime();
      return state.settings;
    }
    settingsPromise = null;
    startupPromise = null;
    const settings = await loadSettings();
    if (!isCurrentInstance()) return null;
    if (!settings?.enabled) {
      pushDiagnostic("settings:disabled-sync", {});
      stopRuntime();
      return settings;
    }
    pushDiagnostic("settings:enabled-sync", {});
    activateRuntime();
    return settings;
  }

  function destroy() {
    state.destroyed = true;
    stopRuntime();
    if (window[API_KEY]?.instanceId === INSTANCE_ID) delete window[API_KEY];
  }

  function escapeHtml(value) {
    return String(value ?? "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#39;");
  }

  function escapeAttr(value) {
    return escapeHtml(value);
  }

  async function start() {
    if (startupPromise) return startupPromise;
    startupPromise = (async () => {
      const settings = await ensureSettings();
      if (!isCurrentInstance()) return;
      if (!settings?.enabled) {
        pushDiagnostic("startup:disabled", {});
        startupPromise = null;
        return;
      }
      activateRuntime();
    })();
    return startupPromise;
  }

  window[API_KEY] = {
    version: SCRIPT_VERSION,
    instanceId: INSTANCE_ID,
    state,
    scan,
    start,
    destroy,
    loadSettings,
    syncSettings,
    renderFloat,
    diagnostics: () => state.diagnostics.slice(),
  };

  void start();
})();
