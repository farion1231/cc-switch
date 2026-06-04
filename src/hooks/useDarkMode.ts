import { useEffect, useState } from "react";

/**
 * Subscribe to the presence of `class="dark"` on `<html>`. The theme-provider
 * toggles this class whenever the effective theme changes (including when the
 * OS changes theme while the app is in "system" mode), so observing the class
 * is the simplest way to drive components that need a boolean `darkMode` prop
 * (e.g. CodeMirror-based editors that don't consume the theme context).
 */
export function useDarkMode(): boolean {
  const [isDark, setIsDark] = useState(() =>
    typeof document !== "undefined"
      ? document.documentElement.classList.contains("dark")
      : false,
  );

  useEffect(() => {
    const observer = new MutationObserver(() => {
      setIsDark(document.documentElement.classList.contains("dark"));
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);

  return isDark;
}
