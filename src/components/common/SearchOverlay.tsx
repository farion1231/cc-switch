import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AnimatePresence, motion } from "framer-motion";
import { Search, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

interface SearchOverlayProps {
  isOpen: boolean;
  searchTerm: string;
  placeholder?: string;
  scopeHint?: string;
  onSearchChange: (term: string) => void;
  onClose: () => void;
  onClear: () => void;
}

// Detect platform for keyboard shortcut display
const isMac =
  typeof navigator !== "undefined" &&
  /Mac|iPod|iPhone|iPad/.test(navigator.platform);

export const SearchOverlay: React.FC<SearchOverlayProps> = ({
  isOpen,
  searchTerm,
  placeholder,
  scopeHint,
  onSearchChange,
  onClose,
  onClear,
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
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

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
            <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted-foreground">
              <span>{scopeHint || defaultScopeHint}</span>
              <span>
                {t("search.closeHint", {
                  defaultValue: "Press Esc to close",
                })}
              </span>
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
export function useSearchShortcut(
  onOpen: () => void,
  shortcut: string = "mod+k",
): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const modPressed = isMac ? event.metaKey : event.ctrlKey;

      // Parse shortcut (e.g., "mod+k" -> mod key + k)
      const parts = shortcut.toLowerCase().split("+");
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
        onOpen();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onOpen, shortcut]);
}

export default SearchOverlay;
