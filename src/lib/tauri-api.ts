import { listen, UnlistenFn } from "@tauri-apps/api/event";

// Tauri API 封装，提供统一的全局 API 接口
export const tauriAPI = {
  // 监听供应商切换事件
  onProviderSwitched: async (
    callback: (data: { appType: string; providerId: string }) => void,
  ): Promise<UnlistenFn> => {
    return await listen("provider-switched", (event) => {
      callback(event.payload as { appType: string; providerId: string });
    });
  },
};

export default tauriAPI;