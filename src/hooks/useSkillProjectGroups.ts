import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { skillsApi, type SkillProjectGroup } from "@/lib/api/skills";

const queryKey = ["skills", "projectGroups"];

export function useSkillProjectGroups() {
  return useQuery({ queryKey, queryFn: skillsApi.getProjectGroups });
}

export function useSaveSkillProjectGroup() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (group: Omit<SkillProjectGroup, "id"> & { id?: string }) =>
      skillsApi.saveProjectGroup(group),
    onSuccess: () => client.invalidateQueries({ queryKey }),
  });
}

export function useDeleteSkillProjectGroup() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => skillsApi.deleteProjectGroup(id),
    onSuccess: () => client.invalidateQueries({ queryKey }),
  });
}
