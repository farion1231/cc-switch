/**
 * CC Switch Codex page enhancement runtime.
 * Contract:
 *   window.__ccSwitchCodex = {
 *     instanceId, version:1,
 *     configure(flags), status(), dispose()
 *   }
 * Idempotent: same instanceId → configure only; different → dispose + remount.
 */
(function () {
  "use strict";

  var VERSION = 1;
  var FEATURE_KEYS = [
    "pluginUnlock",
    "autoExpand",
    "sessionDelete",
    "wideConversation",
    "nativeMenu",
    "userScriptRuntime",
    "markdownExport",
    "modelSwitcher",
    "systemPrompt",
    "reasoningResume",
    "reasoningToken",
  ];

  function emptyStatus() {
    var s = {};
    FEATURE_KEYS.forEach(function (k) {
      s[k] = { state: "disabled" };
    });
    return s;
  }

  function applyFlags(flags, featureFns) {
    var status = emptyStatus();
    FEATURE_KEYS.forEach(function (key) {
      var enabled = !!(flags && flags[key]);
      if (!enabled) {
        status[key] = { state: "disabled" };
        try {
          if (featureFns && typeof featureFns[key] === "function") {
            featureFns[key](false);
          }
        } catch (err) {
          status[key] = {
            state: "failed",
            error: String((err && err.message) || err),
          };
        }
        return;
      }
      try {
        if (featureFns && typeof featureFns[key] === "function") {
          featureFns[key](true);
        }
        status[key] = { state: "loaded" };
      } catch (err) {
        status[key] = {
          state: "failed",
          error: String((err && err.message) || err),
        };
      }
    });
    return status;
  }

  function createRuntime(instanceId, initialFlags, bridgeMeta) {
    var featureFns =
      (typeof window !== "undefined" && window.__ccSwitchCodexFeatures) || {};
    var flags = Object.assign({}, initialFlags || {});
    var lastStatus = applyFlags(flags, featureFns);
    var disposed = false;
    var observer = null;

    // Re-apply on DOM mutations for SPA navigations (lightweight).
    try {
      if (typeof MutationObserver !== "undefined" && document.documentElement) {
        var timer = null;
        observer = new MutationObserver(function () {
          if (disposed) return;
          if (timer) clearTimeout(timer);
          timer = setTimeout(function () {
            lastStatus = applyFlags(flags, featureFns);
          }, 250);
        });
        observer.observe(document.documentElement, {
          childList: true,
          subtree: true,
        });
      }
    } catch (_) {}

    return {
      instanceId: instanceId,
      version: VERSION,
      bridgePort: bridgeMeta && bridgeMeta.bridgePort,
      // nonce intentionally not re-exposed after bootstrap if already set
      configure: function (nextFlags) {
        if (disposed) return;
        flags = Object.assign({}, nextFlags || {});
        lastStatus = applyFlags(flags, featureFns);
      },
      status: function () {
        return Object.assign({}, lastStatus);
      },
      dispose: function () {
        if (disposed) return;
        disposed = true;
        try {
          if (observer) observer.disconnect();
        } catch (_) {}
        // disable all features
        lastStatus = applyFlags({}, featureFns);
        try {
          var style = document.getElementById(
            "cc-switch-codex-enhancement-styles"
          );
          if (style && style.parentNode) style.parentNode.removeChild(style);
        } catch (_) {}
      },
    };
  }

  /**
   * Bootstrap entry used by Rust-built bundle.
   * config: { instanceId, bridgePort, nonce, features }
   */
  function bootstrap(config) {
    config = config || {};
    var instanceId = String(config.instanceId || "unknown");
    var existing =
      typeof window !== "undefined" ? window.__ccSwitchCodex : null;

    if (
      existing &&
      existing.instanceId === instanceId &&
      typeof existing.configure === "function"
    ) {
      existing.configure(config.features || {});
      return existing;
    }

    if (existing && typeof existing.dispose === "function") {
      try {
        existing.dispose();
      } catch (_) {}
    }

    var runtime = createRuntime(instanceId, config.features || {}, {
      bridgePort: config.bridgePort,
    });

    // Secure bridge helper (localhost + Bearer nonce). Secrets like API keys never appear here.
    if (config.bridgePort && config.nonce) {
      runtime.fetchBridge = function (path, init) {
        init = init || {};
        var headers = Object.assign({}, init.headers || {}, {
          Authorization: "Bearer " + config.nonce,
        });
        return fetch(
          "http://127.0.0.1:" + config.bridgePort + (path || "/"),
          Object.assign({}, init, { headers: headers })
        );
      };
    }

    if (typeof window !== "undefined") {
      window.__ccSwitchCodex = runtime;
      window.__ccSwitchCodexBootstrapped = true;
    }
    return runtime;
  }

  // Export for embedding
  if (typeof window !== "undefined") {
    window.__ccSwitchCodexBootstrap = bootstrap;
  }
})();
