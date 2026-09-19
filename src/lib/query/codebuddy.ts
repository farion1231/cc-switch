import { type QueryClient } from "@tanstack/react-query";

/**
 * CodeBuddy / WorkBuddy 配置目录变更后失效相关缓存。
 *
 * 这两个 app 没有轮询，目录变了若不主动失效，供应商页会继续展示旧目录的卡片，
 * 点“启用”就会把陈旧 provider 写进新目录的 models.json；会话视图同理。
 */
export const invalidateCodebuddyDirectoryCaches = async (
  queryClient: QueryClient,
  appId: "codebuddy" | "workbuddy",
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["providers", appId] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};
