import React, {
  useState,
  useEffect,
  useCallback,
  useRef,
  useMemo,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Calendar, Trash2, Plus, Search, X, FolderOpen } from "lucide-react";
import { AnimatePresence, motion } from "framer-motion";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import MarkdownEditor from "@/components/MarkdownEditor";
import {
  workspaceApi,
  type DailyMemoryFileInfo,
  type DailyMemorySearchResult,
} from "@/lib/api/workspace";

interface DailyMemoryPanelProps {
  isOpen: boolean;
  onClose: () => void;
}

function getTodayFilename(): string {
  const now = new Date();
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}.md`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const DailyMemoryPanel: React.FC<DailyMemoryPanelProps> = ({
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation();

  // List state
  const [files, setFiles] = useState<DailyMemoryFileInfo[]>([]);
  const [loadingList, setLoadingList] = useState(false);

  // Edit state
  const [editingFile, setEditingFile] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [loadingContent, setLoadingContent] = useState(false);
  const [saving, setSaving] = useState(false);

  // Delete state
  const [deletingFile, setDeletingFile] = useState<string | null>(null);

  // Search state
  const [searchTerm, setSearchTerm] = useState("");
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchResults, setSearchResults] = useState<DailyMemorySearchResult[]>(
    [],
  );
  const [searching, setSearching] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Dark mode
  const [isDarkMode, setIsDarkMode] = useState(false);

  useEffect(() => {
    setIsDarkMode(document.documentElement.classList.contains("dark"));
    const observer = new MutationObserver(() => {
      setIsDarkMode(document.documentElement.classList.contains("dark"));
    });
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);

  // Whether we are in active search mode (search open with a non-empty term)
  const isActiveSearch = useMemo(
    () => isSearchOpen && searchTerm.trim().length > 0,
    [isSearchOpen, searchTerm],
  );

  // Debounced search execution
  const executeSearch = useCallback(
    async (query: string) => {
      if (!query.trim()) {
        setSearchResults([]);
        setSearching(false);
        return;
      }
      setSearching(true);
      try {
        const results = await workspaceApi.searchDailyMemoryFiles(query.trim());
        setSearchResults(results);
      } catch (err) {
        console.error("Failed to search daily memory files:", err);
        toast.error(t("workspace.dailyMemory.searchFailed"));
      } finally {
        setSearching(false);
      }
    },
    [t],
  );

  // Handle search input change with debounce
  const handleSearchChange = useCallback(
    (value: string) => {
      setSearchTerm(value);
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        void executeSearch(value);
      }, 300);
    },
    [executeSearch],
  );

  // Open search bar
  const openSearch = useCallback(() => {
    setIsSearchOpen(true);
    // Focus input on next frame
    requestAnimationFrame(() => {
      searchInputRef.current?.focus();
    });
  }, []);

  // Close search bar and clear state
  const closeSearch = useCallback(() => {
    setIsSearchOpen(false);
    setSearchTerm("");
    setSearchResults([]);
    setSearching(false);
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
  }, []);

  // Keyboard shortcut: Cmd/Ctrl+F to open search, Escape to close
  useEffect(() => {
    if (!isOpen || editingFile) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "f") {
        e.preventDefault();
        if (!isSearchOpen) {
          openSearch();
        } else {
          searchInputRef.current?.focus();
        }
      }
      if (e.key === "Escape" && isSearchOpen) {
        e.preventDefault();
        closeSearch();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, editingFile, isSearchOpen, openSearch, closeSearch]);

  // Clean up debounce timer on unmount
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  // Load file list
  const loadFiles = useCallback(async () => {
    setLoadingList(true);
    try {
      const list = await workspaceApi.listDailyMemoryFiles();
      setFiles(list);
    } catch (err) {
      console.error("Failed to load daily memory files:", err);
      toast.error(t("workspace.dailyMemory.loadFailed"));
    } finally {
      setLoadingList(false);
    }
  }, [t]);

  useEffect(() => {
    if (isOpen) {
      void loadFiles();
    }
  }, [isOpen, loadFiles]);

  // Open file for editing
  const openFile = useCallback(
    async (filename: string) => {
      setLoadingContent(true);
      setEditingFile(filename);
      try {
        const data = await workspaceApi.readDailyMemoryFile(filename);
        setContent(data ?? "");
      } catch (err) {
        console.error("Failed to read daily memory file:", err);
        toast.error(t("workspace.dailyMemory.loadFailed"));
        setEditingFile(null);
      } finally {
        setLoadingContent(false);
      }
    },
    [t],
  );

  // Create today's note (deferred — file is only persisted on save)
  const handleCreateToday = useCallback(async () => {
    const filename = getTodayFilename();
    // Check if already exists in the list
    const existing = files.find((f) => f.filename === filename);
    if (existing) {
      // Just open it
      await openFile(filename);
      return;
    }
    // Open editor with empty content — no file created until user saves
    setEditingFile(filename);
    setContent("");
  }, [files, openFile]);

  // Save current file
  const handleSave = useCallback(async () => {
    if (!editingFile) return;
    setSaving(true);
    try {
      await workspaceApi.writeDailyMemoryFile(editingFile, content);
      toast.success(t("workspace.saveSuccess"));
    } catch (err) {
      console.error("Failed to save daily memory file:", err);
      toast.error(t("workspace.saveFailed"));
    } finally {
      setSaving(false);
    }
  }, [editingFile, content, t]);

  // Delete file
  const handleDelete = useCallback(async () => {
    if (!deletingFile) return;
    try {
      await workspaceApi.deleteDailyMemoryFile(deletingFile);
      toast.success(t("workspace.dailyMemory.deleteSuccess"));
      setDeletingFile(null);
      // If we were editing this file, go back to list
      if (editingFile === deletingFile) {
        setEditingFile(null);
      }
      await loadFiles();
      // Re-trigger search if active
      if (isSearchOpen && searchTerm.trim()) {
        void executeSearch(searchTerm);
      }
    } catch (err) {
      console.error("Failed to delete daily memory file:", err);
      toast.error(t("workspace.dailyMemory.deleteFailed"));
      setDeletingFile(null);
    }
  }, [
    deletingFile,
    editingFile,
    loadFiles,
    t,
    isSearchOpen,
    searchTerm,
    executeSearch,
  ]);

  // Back from edit mode to list mode — preserve search state
  const handleBackToList = useCallback(() => {
    setEditingFile(null);
    setContent("");
    void loadFiles();
    // Re-trigger search if active (file content may have changed)
    if (isSearchOpen && searchTerm.trim()) {
      void executeSearch(searchTerm);
    }
  }, [loadFiles, isSearchOpen, searchTerm, executeSearch]);

  // Close panel entirely — clear search state
  const handleClose = useCallback(() => {
    setEditingFile(null);
    setContent("");
    setIsSearchOpen(false);
    setSearchTerm("");
    setSearchResults([]);
    setSearching(false);
    onClose();
  }, [onClose]);

  // --- Edit mode ---
  if (editingFile) {
    return (
      <>
        <FullScreenPanel
          isOpen={isOpen}
          title={t("workspace.editing", { filename: editingFile })}
          onClose={handleBackToList}
          footer={
            <Button onClick={handleSave} disabled={saving || loadingContent}>
              {saving ? t("common.saving") : t("common.save")}
            </Button>
          }
        >
          {loadingContent ? (
            <div className="flex items-center justify-center h-64 text-muted-foreground">
              {t("prompts.loading")}
            </div>
          ) : (
            <MarkdownEditor
              value={content}
              onChange={setContent}
              darkMode={isDarkMode}
              placeholder={`# ${editingFile}\n\n...`}
              minHeight="calc(100vh - 240px)"
            />
          )}
        </FullScreenPanel>

        <ConfirmDialog
          isOpen={!!deletingFile}
          title={t("workspace.dailyMemory.confirmDeleteTitle")}
          message={t("workspace.dailyMemory.confirmDeleteMessage", {
            date: deletingFile?.replace(".md", "") ?? "",
          })}
          onConfirm={handleDelete}
          onCancel={() => setDeletingFile(null)}
        />
      </>
    );
  }

  // --- List mode ---
  return (
    <>
      <FullScreenPanel
        isOpen={isOpen}
        title={t("workspace.dailyMemory.title")}
        onClose={handleClose}
      >
        <div className="space-y-4">
          {/* Header with path, search, and create button */}
          <div className="flex items-center justify-between gap-2">
            <p
              className="text-sm text-muted-foreground shrink-0 cursor-pointer hover:text-foreground transition-colors inline-flex items-center gap-1"
              onClick={() => workspaceApi.openDirectory("memory")}
              title={t("workspace.openDirectory")}
            >
              ~/.openclaw/workspace/memory/
              <FolderOpen className="w-3.5 h-3.5" />
            </p>
            <div className="flex items-center gap-1.5">
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8"
                onClick={isSearchOpen ? closeSearch : openSearch}
                title={t("workspace.dailyMemory.searchScopeHint")}
              >
                <Search className="w-4 h-4" />
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={handleCreateToday}
                className="gap-1.5"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("workspace.dailyMemory.createToday")}
              </Button>
            </div>
          </div>

          {/* Search bar */}
          <AnimatePresence>
            {isSearchOpen && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="overflow-hidden"
              >
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-muted-foreground pointer-events-none" />
                    <Input
                      ref={searchInputRef}
                      value={searchTerm}
                      onChange={(e) => handleSearchChange(e.target.value)}
                      placeholder={t("workspace.dailyMemory.searchPlaceholder")}
                      className="pl-8 pr-8 h-8 text-sm"
                    />
                    {searchTerm && (
                      <button
                        onClick={() => handleSearchChange("")}
                        className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
                      >
                        <X className="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={closeSearch}
                    className="text-xs text-muted-foreground h-8 px-2 shrink-0"
                  >
                    {t("workspace.dailyMemory.searchCloseHint")}
                  </Button>
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          {/* Content: search results or normal file list */}
          {isActiveSearch ? (
            // --- Search results ---
            searching ? (
              <div className="flex items-center justify-center h-48 text-muted-foreground">
                {t("workspace.dailyMemory.searching")}
              </div>
            ) : searchResults.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-48 text-muted-foreground gap-3 border-2 border-dashed border-border rounded-xl">
                <Search className="w-10 h-10 opacity-40" />
                <p className="text-sm">
                  {t("workspace.dailyMemory.noSearchResults")}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                {searchResults.map((result) => (
                  <button
                    key={result.filename}
                    onClick={() => openFile(result.filename)}
                    className="w-full flex items-start gap-3 p-4 rounded-xl border border-border bg-card hover:bg-accent/50 transition-colors text-left group"
                  >
                    <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
                      <Calendar className="w-4 h-4" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-sm text-foreground">
                          {result.date}
                        </span>
                        <span className="text-xs text-muted-foreground">
                          {formatFileSize(result.sizeBytes)}
                        </span>
                        {result.matchCount > 0 && (
                          <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-primary/10 text-primary font-medium">
                            {t("workspace.dailyMemory.matchCount", {
                              count: result.matchCount,
                            })}
                          </span>
                        )}
                      </div>
                      {result.snippet && (
                        <p className="text-xs text-muted-foreground mt-1 line-clamp-2 whitespace-pre-line">
                          {result.snippet}
                        </p>
                      )}
                    </div>
                    <div
                      className="opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0"
                      onClick={(e) => {
                        e.stopPropagation();
                        setDeletingFile(result.filename);
                      }}
                    >
                      <Trash2 className="w-4 h-4 text-muted-foreground hover:text-destructive transition-colors" />
                    </div>
                  </button>
                ))}
              </div>
            )
          ) : // --- Normal file list ---
          loadingList ? (
            <div className="flex items-center justify-center h-48 text-muted-foreground">
              {t("prompts.loading")}
            </div>
          ) : files.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 text-muted-foreground gap-3">
              <Calendar className="w-10 h-10 opacity-40" />
              <p className="text-sm">{t("workspace.dailyMemory.empty")}</p>
            </div>
          ) : (
            <div className="space-y-2">
              {files.map((file) => (
                <button
                  key={file.filename}
                  onClick={() => openFile(file.filename)}
                  className="w-full flex items-start gap-3 p-4 rounded-xl border border-border bg-card hover:bg-accent/50 transition-colors text-left group"
                >
                  <div className="mt-0.5 text-muted-foreground group-hover:text-foreground transition-colors">
                    <Calendar className="w-4 h-4" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-sm text-foreground">
                        {file.date}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {formatFileSize(file.sizeBytes)}
                      </span>
                    </div>
                    {file.preview && (
                      <p className="text-xs text-muted-foreground mt-1 line-clamp-2">
                        {file.preview}
                      </p>
                    )}
                  </div>
                  <div
                    className="opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0"
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeletingFile(file.filename);
                    }}
                  >
                    <Trash2 className="w-4 h-4 text-muted-foreground hover:text-destructive transition-colors" />
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>
      </FullScreenPanel>

      <ConfirmDialog
        isOpen={!!deletingFile}
        title={t("workspace.dailyMemory.confirmDeleteTitle")}
        message={t("workspace.dailyMemory.confirmDeleteMessage", {
          date: deletingFile?.replace(".md", "") ?? "",
        })}
        onConfirm={handleDelete}
        onCancel={() => setDeletingFile(null)}
      />
    </>
  );
};

export default DailyMemoryPanel;
