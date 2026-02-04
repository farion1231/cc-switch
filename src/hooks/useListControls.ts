import {
  useCallback,
  useMemo,
  useState,
  useEffect,
  useDeferredValue,
} from "react";

// Types
export type ViewMode = "list" | "card";
export type SortField = "name" | "createdAt" | "custom";
export type SortOrder = "asc" | "desc";

export interface ListControlsState {
  viewMode: ViewMode;
  searchTerm: string;
  sortField: SortField;
  sortOrder: SortOrder;
}

export interface UseListControlsOptions {
  panelId: string;
  defaultViewMode?: ViewMode;
  defaultSortField?: SortField;
  defaultSortOrder?: SortOrder;
}

export interface UseListControlsResult {
  // State
  viewMode: ViewMode;
  searchTerm: string;
  deferredSearchTerm: string; // 防抖后的搜索词
  highlightQuery: string; // 用于高亮的搜索词（去除字段前缀）
  sortField: SortField;
  sortOrder: SortOrder;
  isSearchOpen: boolean;
  isAnonymousMode: boolean;
  searchHistory: string[]; // 搜索历史

  // Actions
  setViewMode: (mode: ViewMode) => void;
  setSearchTerm: (term: string) => void;
  setSortField: (field: SortField) => void;
  setSortOrder: (order: SortOrder) => void;
  toggleSortOrder: () => void;
  openSearch: () => void;
  closeSearch: () => void;
  clearSearch: () => void;
  toggleAnonymousMode: () => void;
  addToSearchHistory: (term: string) => void;
  clearSearchHistory: () => void;
  selectFromHistory: (term: string) => void;

  // Computed
  filterItems: <T extends FilterableItem>(items: T[]) => T[];
  sortItems: <T extends SortableItem>(items: T[]) => T[];
}

// Item interfaces for filtering and sorting
export interface FilterableItem {
  name: string;
  description?: string;
  tags?: string[];
  notes?: string;
  baseUrl?: string; // Universal Provider URL
  websiteUrl?: string; // Provider URL
}

export interface SortableItem {
  name: string;
  createdAt?: number;
  sortIndex?: number;
}

// Storage key prefix
const STORAGE_KEY_PREFIX = "cc-switch:list-controls:";
const HISTORY_KEY_PREFIX = "cc-switch:search-history:";
const MAX_HISTORY_SIZE = 8;

// Get storage key for a panel
const getStorageKey = (panelId: string): string =>
  `${STORAGE_KEY_PREFIX}${panelId}`;
const getHistoryKey = (panelId: string): string =>
  `${HISTORY_KEY_PREFIX}${panelId}`;

// Load persisted state from localStorage
const loadPersistedState = (
  panelId: string,
  defaults: {
    viewMode: ViewMode;
    sortField: SortField;
    sortOrder: SortOrder;
    isAnonymousMode: boolean;
  },
): {
  viewMode: ViewMode;
  sortField: SortField;
  sortOrder: SortOrder;
  isAnonymousMode: boolean;
} => {
  try {
    const stored = localStorage.getItem(getStorageKey(panelId));
    if (stored) {
      const parsed = JSON.parse(stored);
      return {
        viewMode: parsed.viewMode ?? defaults.viewMode,
        sortField: parsed.sortField ?? defaults.sortField,
        sortOrder: parsed.sortOrder ?? defaults.sortOrder,
        isAnonymousMode: parsed.isAnonymousMode ?? defaults.isAnonymousMode,
      };
    }
  } catch (error) {
    console.warn(
      `[useListControls] Failed to load persisted state for ${panelId}:`,
      error,
    );
  }
  return defaults;
};

// Save state to localStorage
const savePersistedState = (
  panelId: string,
  state: {
    viewMode: ViewMode;
    sortField: SortField;
    sortOrder: SortOrder;
    isAnonymousMode: boolean;
  },
): void => {
  try {
    localStorage.setItem(getStorageKey(panelId), JSON.stringify(state));
  } catch (error) {
    console.warn(
      `[useListControls] Failed to save persisted state for ${panelId}:`,
      error,
    );
  }
};

// Load search history from localStorage
const loadSearchHistory = (panelId: string): string[] => {
  try {
    const stored = localStorage.getItem(getHistoryKey(panelId));
    return stored ? JSON.parse(stored) : [];
  } catch {
    return [];
  }
};

// Save search history to localStorage
const saveSearchHistory = (panelId: string, history: string[]): void => {
  try {
    localStorage.setItem(getHistoryKey(panelId), JSON.stringify(history));
  } catch (error) {
    console.warn(
      `[useListControls] Failed to save search history for ${panelId}:`,
      error,
    );
  }
};

/**
 * 解析搜索语法
 * 支持: name:xxx, tag:xxx, note:xxx, url:xxx
 */
interface ParsedSearch {
  field: "name" | "tag" | "note" | "url" | null;
  value: string;
}

function parseSearchTerm(term: string): ParsedSearch {
  const normalizedTerm = term.toLowerCase().trim();
  const fieldMatch = normalizedTerm.match(/^(name|tag|note|url):(.+)$/);
  if (fieldMatch) {
    return {
      field: fieldMatch[1] as ParsedSearch["field"],
      value: fieldMatch[2].trim(),
    };
  }
  return { field: null, value: normalizedTerm };
}

/**
 * useListControls - Hook for managing view mode, search, and sort state
 *
 * Features:
 * - View mode switching (list/card) with persistence
 * - Search filtering (session-only, not persisted)
 * - Sorting by name, createdAt, or custom order with persistence
 */
export function useListControls(
  options: UseListControlsOptions,
): UseListControlsResult {
  const {
    panelId,
    defaultViewMode = "list",
    defaultSortField = "custom",
    defaultSortOrder = "asc",
  } = options;

  // Load initial state from localStorage
  const initialState = useMemo(
    () =>
      loadPersistedState(panelId, {
        viewMode: defaultViewMode,
        sortField: defaultSortField,
        sortOrder: defaultSortOrder,
        isAnonymousMode: false,
      }),
    [panelId, defaultViewMode, defaultSortField, defaultSortOrder],
  );

  // Persisted state
  const [viewMode, setViewModeState] = useState<ViewMode>(
    initialState.viewMode,
  );
  const [sortField, setSortFieldState] = useState<SortField>(
    initialState.sortField,
  );
  const [sortOrder, setSortOrderState] = useState<SortOrder>(
    initialState.sortOrder,
  );
  const [isAnonymousMode, setIsAnonymousMode] = useState(
    initialState.isAnonymousMode,
  );

  // Session-only state (not persisted)
  const [searchTerm, setSearchTerm] = useState("");
  const [isSearchOpen, setIsSearchOpen] = useState(false);

  // 搜索防抖 - 使用 useDeferredValue
  const deferredSearchTerm = useDeferredValue(searchTerm);

  // 搜索历史
  const [searchHistory, setSearchHistory] = useState<string[]>(() =>
    loadSearchHistory(panelId),
  );

  // Persist state changes
  useEffect(() => {
    savePersistedState(panelId, {
      viewMode,
      sortField,
      sortOrder,
      isAnonymousMode,
    });
  }, [panelId, viewMode, sortField, sortOrder, isAnonymousMode]);

  // Actions
  const setViewMode = useCallback((mode: ViewMode) => {
    setViewModeState(mode);
  }, []);

  const setSortField = useCallback((field: SortField) => {
    setSortFieldState(field);
  }, []);

  const setSortOrder = useCallback((order: SortOrder) => {
    setSortOrderState(order);
  }, []);

  const toggleSortOrder = useCallback(() => {
    setSortOrderState((prev) => (prev === "asc" ? "desc" : "asc"));
  }, []);

  const openSearch = useCallback(() => {
    setIsSearchOpen(true);
  }, []);

  const closeSearch = useCallback(() => {
    setIsSearchOpen(false);
    setSearchTerm("");
  }, []);

  const clearSearch = useCallback(() => {
    setSearchTerm("");
  }, []);

  const toggleAnonymousMode = useCallback(() => {
    setIsAnonymousMode((prev) => !prev);
  }, []);

  // 搜索历史操作
  const addToSearchHistory = useCallback(
    (term: string) => {
      const trimmed = term.trim();
      if (!trimmed) return;
      setSearchHistory((prev) => {
        const filtered = prev.filter(
          (t) => t.toLowerCase() !== trimmed.toLowerCase(),
        );
        const updated = [trimmed, ...filtered].slice(0, MAX_HISTORY_SIZE);
        saveSearchHistory(panelId, updated);
        return updated;
      });
    },
    [panelId],
  );

  const clearSearchHistory = useCallback(() => {
    setSearchHistory([]);
    saveSearchHistory(panelId, []);
  }, [panelId]);

  const selectFromHistory = useCallback((term: string) => {
    setSearchTerm(term);
  }, []);

  // Filter items by search term (case-insensitive, with field syntax support)
  const filterItems = useCallback(
    <T extends FilterableItem>(items: T[]): T[] => {
      const normalizedTerm = deferredSearchTerm.toLowerCase().trim();
      if (!normalizedTerm) return items;

      // 解析搜索语法
      const { field, value } = parseSearchTerm(deferredSearchTerm);

      // 如果只输入了前缀（如 "name:"）但没有值，显示所有结果
      if (field && !value) return items;

      if (field && value) {
        // 字段特定搜索
        return items.filter((item) => {
          switch (field) {
            case "name":
              return item.name.toLowerCase().includes(value);
            case "tag":
              return item.tags?.some((tag) =>
                tag.toLowerCase().includes(value),
              );
            case "note":
              return item.notes?.toLowerCase().includes(value);
            case "url":
              // 支持 baseUrl 和 websiteUrl 两种字段
              return (
                item.baseUrl?.toLowerCase().includes(value) ||
                item.websiteUrl?.toLowerCase().includes(value)
              );
            default:
              return false;
          }
        });
      }

      // 全局搜索（匹配所有字段）
      return items.filter((item) => {
        // Match against name
        if (item.name.toLowerCase().includes(normalizedTerm)) return true;
        // Match against description
        if (item.description?.toLowerCase().includes(normalizedTerm))
          return true;
        // Match against notes
        if (item.notes?.toLowerCase().includes(normalizedTerm)) return true;
        // Match against tags
        if (
          item.tags?.some((tag) => tag.toLowerCase().includes(normalizedTerm))
        )
          return true;
        // Match against URL (both baseUrl and websiteUrl)
        if (item.baseUrl?.toLowerCase().includes(normalizedTerm)) return true;
        if (item.websiteUrl?.toLowerCase().includes(normalizedTerm))
          return true;
        return false;
      });
    },
    [deferredSearchTerm],
  );

  // Sort items by selected field and order
  const sortItems = useCallback(
    <T extends SortableItem>(items: T[]): T[] => {
      const sorted = [...items].sort((a, b) => {
        let comparison = 0;

        switch (sortField) {
          case "name":
            // Case-insensitive alphabetical sorting
            comparison = a.name
              .toLowerCase()
              .localeCompare(b.name.toLowerCase());
            break;

          case "createdAt":
            // Sort by creation time (items without createdAt treated as oldest)
            const timeA = a.createdAt ?? 0;
            const timeB = b.createdAt ?? 0;
            comparison = timeA - timeB;
            break;

          case "custom":
          default:
            // Sort by sortIndex (items without sortIndex go to end)
            const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
            const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
            comparison = indexA - indexB;
            // If both have no sortIndex, fall back to name
            if (
              indexA === Number.MAX_SAFE_INTEGER &&
              indexB === Number.MAX_SAFE_INTEGER
            ) {
              comparison = a.name
                .toLowerCase()
                .localeCompare(b.name.toLowerCase());
            }
            break;
        }

        // Apply sort order
        return sortOrder === "asc" ? comparison : -comparison;
      });

      return sorted;
    },
    [sortField, sortOrder],
  );

  // 计算用于高亮的搜索词（去除字段前缀）
  const highlightQuery = useMemo(() => {
    const { value } = parseSearchTerm(deferredSearchTerm);
    return value;
  }, [deferredSearchTerm]);

  return {
    // State
    viewMode,
    searchTerm,
    deferredSearchTerm,
    highlightQuery,
    sortField,
    sortOrder,
    isSearchOpen,
    isAnonymousMode,
    searchHistory,

    // Actions
    setViewMode,
    setSearchTerm,
    setSortField,
    setSortOrder,
    toggleSortOrder,
    openSearch,
    closeSearch,
    clearSearch,
    toggleAnonymousMode,
    addToSearchHistory,
    clearSearchHistory,
    selectFromHistory,

    // Computed
    filterItems,
    sortItems,
  };
}
