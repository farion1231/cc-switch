/**
 * Codex page enhancement features (DOM-level).
 * Each feature is isolated; failures do not break the runtime.
 * Source note: adapted patterns from CodexElves (MIT), rewritten for CC Switch.
 */
(function (root) {
  "use strict";

  var STYLE_ID = "cc-switch-codex-enhancement-styles";

  function ensureStyle(css) {
    var el = document.getElementById(STYLE_ID);
    if (!el) {
      el = document.createElement("style");
      el.id = STYLE_ID;
      (document.head || document.documentElement).appendChild(el);
    }
    el.textContent = css;
  }

  function setCssVar(name, value) {
    try {
      document.documentElement.style.setProperty(name, value);
    } catch (_) {}
  }

  function removeCssVar(name) {
    try {
      document.documentElement.style.removeProperty(name);
    } catch (_) {}
  }

  /** Plugin market unlock: reveal hidden plugin entry points */
  function pluginUnlock(enable) {
    if (!enable) {
      document
        .querySelectorAll("[data-cc-plugin-unlock]")
        .forEach(function (n) {
          n.removeAttribute("data-cc-plugin-unlock");
        });
      return;
    }
    var candidates = document.querySelectorAll(
      '[data-testid*="plugin"], [class*="plugin"], a[href*="plugin"], button[aria-label*="Plugin"]'
    );
    candidates.forEach(function (el) {
      el.setAttribute("data-cc-plugin-unlock", "1");
      if (el.style && el.style.display === "none") {
        el.style.display = "";
      }
      if (el.hasAttribute("hidden")) el.removeAttribute("hidden");
      if (el.getAttribute("aria-hidden") === "true") {
        el.setAttribute("aria-hidden", "false");
      }
    });
  }

  /** Auto-expand collapsed panels / reasoning blocks */
  function autoExpand(enable) {
    if (!enable) return;
    var collapsed = document.querySelectorAll(
      'details:not([open]), [aria-expanded="false"], [data-collapsed="true"]'
    );
    collapsed.forEach(function (el) {
      try {
        if (el.tagName === "DETAILS") el.open = true;
        if (el.getAttribute("aria-expanded") === "false") {
          el.setAttribute("aria-expanded", "true");
        }
        if (el.getAttribute("data-collapsed") === "true") {
          el.setAttribute("data-collapsed", "false");
        }
      } catch (_) {}
    });
  }

  /** Session delete affordance helper */
  function sessionDelete(enable) {
    document.documentElement.toggleAttribute("data-cc-session-delete", !!enable);
  }

  /** Wide conversation view */
  function wideConversation(enable) {
    if (enable) {
      ensureStyle(
        "html[data-cc-wide-conversation] main," +
          "html[data-cc-wide-conversation] [class*='conversation']," +
          "html[data-cc-wide-conversation] [class*='chat-panel']," +
          "html[data-cc-wide-conversation] [class*='thread']{" +
          "max-width:100%!important;width:100%!important;}"
      );
      document.documentElement.setAttribute("data-cc-wide-conversation", "1");
      setCssVar("--cc-conversation-max-width", "100%");
    } else {
      document.documentElement.removeAttribute("data-cc-wide-conversation");
      removeCssVar("--cc-conversation-max-width");
    }
  }

  /** Native menu position tweak */
  function nativeMenu(enable) {
    document.documentElement.toggleAttribute("data-cc-native-menu", !!enable);
    if (enable) {
      ensureStyle(
        "html[data-cc-native-menu] [role='menu']," +
          "html[data-cc-native-menu] [data-radix-menu-content]{" +
          "z-index:2147483000!important;}"
      );
    }
  }

  /** User script runtime host marker */
  function userScriptRuntime(enable) {
    document.documentElement.toggleAttribute(
      "data-cc-user-script-runtime",
      !!enable
    );
  }

  /** Markdown export button marker (UI only; export logic later) */
  function markdownExport(enable) {
    document.documentElement.toggleAttribute(
      "data-cc-markdown-export",
      !!enable
    );
  }

  /** Model switcher helper */
  function modelSwitcher(enable) {
    document.documentElement.toggleAttribute(
      "data-cc-model-switcher",
      !!enable
    );
  }

  /** System prompt / reasoning markers (proxy-side; DOM flags only here) */
  function systemPrompt(enable) {
    document.documentElement.toggleAttribute("data-cc-system-prompt", !!enable);
  }
  function reasoningResume(enable) {
    document.documentElement.toggleAttribute(
      "data-cc-reasoning-resume",
      !!enable
    );
  }
  function reasoningToken(enable) {
    document.documentElement.toggleAttribute(
      "data-cc-reasoning-token",
      !!enable
    );
  }

  var FEATURES = {
    pluginUnlock: pluginUnlock,
    autoExpand: autoExpand,
    sessionDelete: sessionDelete,
    wideConversation: wideConversation,
    nativeMenu: nativeMenu,
    userScriptRuntime: userScriptRuntime,
    markdownExport: markdownExport,
    modelSwitcher: modelSwitcher,
    systemPrompt: systemPrompt,
    reasoningResume: reasoningResume,
    reasoningToken: reasoningToken,
  };

  root.__ccSwitchCodexFeatures = FEATURES;
})(typeof window !== "undefined" ? window : globalThis);
