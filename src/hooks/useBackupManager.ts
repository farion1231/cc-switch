import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { backupsApi } from "@/lib/api";

export function useBackupManager() {
  const queryClient = useQueryClient();

  const {
    data: backups = [],
    isLoading,
    refetch,
  } = useQuery({
    queryKey: ["db-backups"],
    queryFn: () => backupsApi.listDbBackups(),
  });

  const createMutation = useMutation({
    mutationFn: () => backupsApi.createDbBackup(),
    onSuccess: () => refetch(),
  });

  const previewMutation = useMutation({
    mutationFn: (filename: string) =>
      backupsApi.previewDbBackupRestore(filename),
  });

  const restoreMutation = useMutation({
    mutationFn: ({
      filename,
      restoreToken,
      preserveLocalPreferences,
    }: {
      filename: string;
      restoreToken: string;
      preserveLocalPreferences: boolean;
    }) =>
      backupsApi.restoreDbBackup(
        filename,
        restoreToken,
        preserveLocalPreferences,
      ),
    onSuccess: () => queryClient.invalidateQueries(),
  });

  const renameMutation = useMutation({
    mutationFn: ({
      oldFilename,
      newName,
    }: {
      oldFilename: string;
      newName: string;
    }) => backupsApi.renameDbBackup(oldFilename, newName),
    onSuccess: () => refetch(),
  });

  const deleteMutation = useMutation({
    mutationFn: (filename: string) => backupsApi.deleteDbBackup(filename),
    onSuccess: () => refetch(),
  });

  return {
    backups,
    isLoading,
    create: createMutation.mutateAsync,
    isCreating: createMutation.isPending,
    previewRestore: previewMutation.mutateAsync,
    isPreviewingRestore: previewMutation.isPending,
    restore: restoreMutation.mutateAsync,
    isRestoring: restoreMutation.isPending,
    rename: renameMutation.mutateAsync,
    isRenaming: renameMutation.isPending,
    remove: deleteMutation.mutateAsync,
    isDeleting: deleteMutation.isPending,
  };
}
