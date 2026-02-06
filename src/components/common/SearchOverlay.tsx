import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { Search, X, Clock, Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

interface SearchOverlayProps {
  isOpen: boolean;
  searchTerm: string;
  placeholder?: string;
  scopeHint?: string;
  resultCount?: number;
  totalCount?: number;
  searchHistory?: string[];
  onSearchChange: (term: string) => void;
  onClose: () => void;
  onClear: () => void;
  onSelectHistory?: (term: string) => void;
  onClearHistory?: () => void;
  onSearchSubmit?: (term: string) => void;
}

// 搜索语法前缀
const SEARCH_SYNTAX_PREFIXES = ["name:", "url:"] as const;

// Detect platform for keyboard shortcut display
const isMac =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);

export const SearchOverlay: React.FC<SearchOverlayProps> = ({
  isOpen,
  searchTerm,
  placeholder,
  scopeHint,
  resultCount,
  totalCount,
  searchHistory = [],
  onSearchChange,
  onClose,
  onClear,
  onSelectHistory,
  onClearHistory,
  onSearchSubmit,
}) => {
  const { t } = useTranslation();
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Focus input when overlay opens
  useEffect(() => {
    if (isOpen) {
      const frame = requestAnimationFrame(() => {
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [isOpen]);

  // Handle Escape key
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && isOpen) {
        event.preventDefault();
        onClose();
      }
      // Enter 键提交搜索（添加到历史）
      if (event.key === "Enter" && isOpen && searchTerm.trim()) {
        if (event.isComposing || (event as KeyboardEvent).keyCode === 229) {
          return;
        }
        event.preventDefault();
        onSearchSubmit?.(searchTerm.trim());
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose, searchTerm, onSearchSubmit]);

  const defaultPlaceholder = t("search.placeholder", {
    defaultValue: "Search...",
  });

  const defaultScopeHint = t("search.scopeHint", {
    defaultValue: "Matches name, description, and tags.",
  });

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          {/* Backdrop for click-outside-to-close */}
          <motion.div
            key="search-backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="fixed inset-0 z-30"
            onClick={onClose}
            aria-hidden="true"
          />
          <motion.div
            key="search-overlay"
            initial={{ opacity: 0, y: -8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -8, scale: 0.98 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="fixed left-1/2 top-[6.5rem] z-40 w-[min(90vw,26rem)] -translate-x-1/2 sm:right-6 sm:left-auto sm:translate-x-0"
          >
            <div className="p-4 space-y-3 border shadow-md rounded-2xl border-white/10 bg-background/95 shadow-black/20 backdrop-blur-md">
              <div className="relative flex items-center gap-2">
                <Search className="absolute w-4 h-4 -translate-y-1/2 pointer-events-none left-3 top-1/2 text-muted-foreground" />
                <Input
                  ref={searchInputRef}
                  value={searchTerm}
                  onChange={(event) => onSearchChange(event.target.value)}
                  placeholder={placeholder || defaultPlaceholder}
                  aria-label={t("search.ariaLabel", {
                    defaultValue: "Search items",
                  })}
                  className="pr-16 pl-9"
                />
                {searchTerm && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="absolute text-xs -translate-y-1/2 right-11 top-1/2"
                    onClick={onClear}
                  >
                    {t("common.clear", { defaultValue: "Clear" })}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  size="icon"
                  className="ml-auto"
                  onClick={onClose}
                  aria-label={t("search.closeAriaLabel", {
                    defaultValue: "Close search",
                  })}
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>

              {/* 搜索历史 */}
              {!searchTerm && searchHistory.length > 0 && (
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] text-muted-foreground flex items-center gap-1">
                      <Clock className="w-3 h-3" />
                      {t("search.recentSearches", { defaultValue: "Recent" })}
                    </span>
                    {onClearHistory && (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-5 px-1 text-[10px] text-muted-foreground hover:text-destructive"
                        onClick={onClearHistory}
                      >
                        <Trash2 className="w-3 h-3 mr-1" />
                        {t("search.clearHistory", { defaultValue: "Clear" })}
                      </Button>
                    )}
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {searchHistory.slice(0, 6).map((term, index) => (
                      <button
                        key={index}
                        type="button"
                        className="px-2 py-0.5 text-xs rounded-md bg-muted hover:bg-muted/80 text-muted-foreground hover:text-foreground transition-colors"
                        onClick={() => onSelectHistory?.(term)}
                      >
                        {term}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* 搜索语法提示 */}
              {!searchTerm && (
                <div className="pt-1 border-t border-border/50">
                  <p className="text-[10px] text-muted-foreground/70 mb-1">
                    {t("search.syntaxHint", { defaultValue: "Search syntax:" })}
                  </p>
                  <div className="flex flex-wrap gap-1.5 text-[10px]">
                    {SEARCH_SYNTAX_PREFIXES.map((prefix) => (
                      <button
                        key={prefix}
                        type="button"
                        className="px-1.5 py-0.5 rounded bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors cursor-pointer"
                        onClick={() => {
                          onSearchChange(prefix);
                          // 聚焦输入框并将光标移到末尾
                          searchInputRef.current?.focus();
                        }}
                      >
                        {prefix}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
                <span>{scopeHint || defaultScopeHint}</span>
                <div className="flex items-center gap-3">
                  {/* 搜索结果计数 */}
                  {searchTerm &&
                    resultCount !== undefined &&
                    totalCount !== undefined && (
                      <span className="font-medium">
                        {resultCount} / {totalCount}
                      </span>
                    )}
                  <span>
                    {t("search.closeHint", {
                      defaultValue: "Press Esc to close",
                    })}
                  </span>
                </div>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
};

/**
 * Hook to handle global keyboard shortcut for opening search
 * Default shortcut: Cmd+K (Mac) / Ctrl+K (Windows)
 * @param onOpen - Callback to open search
 * @param shortcut - Shortcut string (e.g., "mod+k"). Defaults to "mod+k"
 */
interface SearchShortcutOptions {
  isOpen?: boolean;
  onClose?: () => void;
}

export function useSearchShortcut(
  onOpen: () => void,
  shortcut: string = "mod+k",
  options?: SearchShortcutOptions,
): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const modPressed = isMac ? event.metaKey : event.ctrlKey;

      // Parse shortcut (e.g., "mod+k" -> mod key + k)
      const parts = shortcut
        .toLowerCase()
        .split("+")
        .map((part) => {
          if (["cmd", "command", "meta", "ctrl", "control"].includes(part)) {
            return "mod";
          }
          if (part === "option") return "alt";
          return part;
        });
      const targetKey = parts[parts.length - 1];
      const needsMod = parts.includes("mod");
      const needsShift = parts.includes("shift");
      const needsAlt = parts.includes("alt");

      // Check all modifiers
      if (needsMod && !modPressed) return;
      if (needsShift && !event.shiftKey) return;
      if (needsAlt && !event.altKey) return;

      if (key === targetKey) {
        event.preventDefault();
        if (options?.isOpen && options.onClose) {
          options.onClose();
        } else {
          onOpen();
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onOpen, shortcut, options?.isOpen, options?.onClose]);
}

export default SearchOverlay;
