import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: true,
      staleTime: 0,
    },
    mutations: {
      retry: false,
    },
  },
});

// 用量命名空间不做聚焦重取：它有事件驱动的失效桥（usage-log-recorded，前端
// 已做 5s 合并）+ 60s 滚动重取兜底，聚焦时的整组重取只会在系统卡顿恢复的
// 瞬间制造一次 IPC 与重渲染惊群，正是渲染进程最不该忙的时刻。
// 键用字面量而不 import usageKeys：queryClient 被几乎所有测试的公共夹具加载，
// 而 usage 模块在不少测试里被整体 mock，保持解耦免得每个 mock 都得补导出。
// 与 `usageKeys.all`（src/lib/query/usage.ts）保持一致。
queryClient.setQueryDefaults(["usage"], {
  refetchOnWindowFocus: false,
});
