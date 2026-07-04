import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Check,
  ChevronsUpDown,
  FolderCog,
  FolderOpen,
  Plus,
  X,
} from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api/types";
import {
  useApplyProfileMutation,
  useClearProfileMutation,
  useCreateProfileMutation,
  useProfilesQuery,
} from "@/lib/query/profiles";
import { ProfileManageDialog } from "./ProfileManageDialog";

/** 后端 services/profile.rs 的 PROFILE_APPS 前端镜像，扩展支持范围时两处同步 */
const PROFILE_SUPPORTED_APPS: AppId[] = ["claude", "claude-desktop", "codex"];

interface ProfileSwitcherProps {
  activeApp: AppId;
}

/**
 * 项目 Profile 切换器（header 左侧入口）
 *
 * Profile 是跨应用的配置快照（Claude Code + Codex 的供应商/MCP/Skills/记忆文件，
 * 以及 Claude Desktop 的供应商），与右侧 AppSwitcher（仅切换查看的应用）语义不同。
 */
export function ProfileSwitcher({ activeApp }: ProfileSwitcherProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isManageOpen, setIsManageOpen] = useState(false);
  const [newName, setNewName] = useState("");

  const { data } = useProfilesQuery();
  const applyMutation = useApplyProfileMutation();
  const clearMutation = useClearProfileMutation();
  const createMutation = useCreateProfileMutation();

  // Profile 仅作用于受支持的应用——在其他应用的标签页展示会误导用户
  // 以为当前应用也被切换了，因此只在受支持应用的页面渲染
  if (!PROFILE_SUPPORTED_APPS.includes(activeApp)) {
    return null;
  }

  const profiles = data?.profiles ?? [];
  const currentId = data?.currentId ?? null;
  const currentProfile = profiles.find((p) => p.id === currentId);

  const handleApply = (id: string) => {
    setOpen(false);
    if (id !== currentId) {
      applyMutation.mutate(id);
    }
  };

  const closeCreateDialog = () => {
    setIsCreateOpen(false);
    setNewName("");
  };

  const handleCreate = () => {
    const name = newName.trim();
    if (!name) return;
    createMutation.mutate(name, { onSuccess: closeCreateDialog });
  };

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            aria-expanded={open}
            title={t("profiles.switcherTooltip")}
            className={cn(
              "inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-sm font-medium transition-colors",
              "hover:bg-black/5 dark:hover:bg-white/5",
              currentProfile ? "text-foreground" : "text-muted-foreground",
            )}
          >
            <FolderOpen className="h-4 w-4 shrink-0 opacity-70" />
            <span className="max-w-[9rem] truncate">
              {currentProfile?.name ?? t("profiles.none")}
            </span>
            <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="bottom"
          align="start"
          sideOffset={6}
          className="z-[100] w-64 p-0"
        >
          <Command>
            <CommandInput placeholder={t("profiles.searchPlaceholder")} />
            <CommandList>
              <CommandEmpty>{t("profiles.empty")}</CommandEmpty>
              {profiles.length > 0 && (
                <CommandGroup>
                  {profiles.map((profile) => (
                    <CommandItem
                      key={profile.id}
                      value={profile.id}
                      keywords={[profile.name]}
                      onSelect={() => handleApply(profile.id)}
                    >
                      <Check
                        className={cn(
                          "mr-2 h-4 w-4 shrink-0",
                          currentId === profile.id
                            ? "opacity-100"
                            : "opacity-0",
                        )}
                      />
                      <span className="truncate">{profile.name}</span>
                    </CommandItem>
                  ))}
                </CommandGroup>
              )}
              <div className="mx-1 my-1 h-px bg-border" />
              <CommandGroup>
                <CommandItem
                  value="__create__"
                  keywords={[t("profiles.createFromCurrent")]}
                  onSelect={() => {
                    setOpen(false);
                    setIsCreateOpen(true);
                  }}
                >
                  <Plus className="mr-2 h-4 w-4 shrink-0" />
                  {t("profiles.createFromCurrent")}
                </CommandItem>
                {currentId && (
                  <CommandItem
                    value="__clear__"
                    keywords={[t("profiles.none")]}
                    onSelect={() => {
                      setOpen(false);
                      clearMutation.mutate();
                    }}
                  >
                    <X className="mr-2 h-4 w-4 shrink-0" />
                    {t("profiles.none")}
                  </CommandItem>
                )}
                {profiles.length > 0 && (
                  <CommandItem
                    value="__manage__"
                    keywords={[t("profiles.manage")]}
                    onSelect={() => {
                      setOpen(false);
                      setIsManageOpen(true);
                    }}
                  >
                    <FolderCog className="mr-2 h-4 w-4 shrink-0" />
                    {t("profiles.manage")}
                  </CommandItem>
                )}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      <Dialog
        open={isCreateOpen}
        onOpenChange={(open) => {
          if (!open) closeCreateDialog();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle>{t("profiles.createFromCurrent")}</DialogTitle>
            <DialogDescription>
              {t("profiles.createDescription")}
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 pt-3">
            <Input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t("profiles.namePlaceholder")}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreate();
              }}
            />
          </div>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={closeCreateDialog}>
              {t("common.cancel")}
            </Button>
            <Button
              onClick={handleCreate}
              disabled={!newName.trim() || createMutation.isPending}
            >
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ProfileManageDialog
        isOpen={isManageOpen}
        onClose={() => setIsManageOpen(false)}
      />
    </>
  );
}
