import { useState, useEffect } from "react";

export function useDarkMode() {
  // 初始设为 false，挂载后在 useEffect 中加载真实值
  const [isDarkMode, setIsDarkMode] = useState<boolean>(false);
  const [isInitialized, setIsInitialized] = useState(false);
  const isDev = import.meta.env.DEV;

  // 组件挂载后加载初始值（兼容 Tauri 环境）
  useEffect(() => {
    if (typeof window === "undefined") return;

    try {
      // 尝试读取已保存的偏好
      const saved = localStorage.getItem("darkMode");
      if (saved !== null) {
        const savedBool = saved === "true";
        setIsDarkMode(savedBool);
        if (isDev)
          console.log("[DarkMode] Loaded from localStorage:", savedBool);
      } else {
        // 回退到系统偏好
        const prefersDark =
          window.matchMedia &&
          window.matchMedia("(prefers-color-scheme: dark)").matches;
        setIsDarkMode(prefersDark);
        if (isDev)
          console.log("[DarkMode] Using system preference:", prefersDark);
      }
    } catch (error) {
      console.error("[DarkMode] Error loading preference:", error);
      setIsDarkMode(false);
    }

    setIsInitialized(true);
  }, []); // 仅在首次挂载时运行

  // 将 dark 类应用到文档根节点
  useEffect(() => {
    if (!isInitialized) return;

    // 添加短暂延迟以确保 Tauri 中 DOM 已就绪
    const timer = setTimeout(() => {
      try {
        if (isDarkMode) {
          document.documentElement.classList.add("dark");
          if (isDev) console.log("[DarkMode] Added dark class to document");
        } else {
          document.documentElement.classList.remove("dark");
          if (isDev) console.log("[DarkMode] Removed dark class from document");
        }

        // 检查类名是否已成功应用
        const hasClass = document.documentElement.classList.contains("dark");
        if (isDev) console.log("[DarkMode] Document has dark class:", hasClass);
      } catch (error) {
        console.error("[DarkMode] Error applying dark class:", error);
      }
    }, 0);

    return () => clearTimeout(timer);
  }, [isDarkMode, isInitialized]);

  // 将偏好保存到 localStorage
  useEffect(() => {
    if (!isInitialized) return;

    try {
      localStorage.setItem("darkMode", isDarkMode.toString());
      if (isDev) console.log("[DarkMode] Saved to localStorage:", isDarkMode);
    } catch (error) {
      console.error("[DarkMode] Error saving preference:", error);
    }
  }, [isDarkMode, isInitialized]);

  const toggleDarkMode = () => {
    setIsDarkMode((prev) => {
      const newValue = !prev;
      if (isDev) console.log("[DarkMode] Toggling from", prev, "to", newValue);
      return newValue;
    });
  };

  return { isDarkMode, toggleDarkMode };
}
