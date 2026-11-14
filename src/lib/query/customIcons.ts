import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { customIconsApi } from "@/lib/api/customIcons";

export const customIconKeys = {
  all: ["customIcons"] as const,
};

export function useCustomIconsQuery() {
  return useQuery({
    queryKey: customIconKeys.all,
    queryFn: () => customIconsApi.list(),
  });
}

export function useAddCustomIconMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, data }: { name: string; data: string }) =>
      customIconsApi.add(name, data),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: customIconKeys.all }),
  });
}

export function useRenameCustomIconMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      customIconsApi.rename(id, name),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: customIconKeys.all }),
  });
}

export function useDeleteCustomIconMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => customIconsApi.remove(id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: customIconKeys.all }),
  });
}
