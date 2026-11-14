import { invoke } from "@tauri-apps/api/core";

/** 「我的图标」里的一项，`data` 和供应商 `icon` 里的自定义图标同一种格式 */
export interface CustomIconEntry {
  id: string;
  name: string;
  data: string;
  createdAt: number;
}

export interface AddCustomIconResult {
  icon: CustomIconEntry;
  /** 同一张图已经在库里：返回的是原条目，名称没有改 */
  existed: boolean;
}

export const customIconsApi = {
  async list(): Promise<CustomIconEntry[]> {
    return await invoke("get_custom_icons");
  },

  async add(name: string, data: string): Promise<AddCustomIconResult> {
    return await invoke("add_custom_icon", { name, data });
  },

  async rename(id: string, name: string): Promise<CustomIconEntry> {
    return await invoke("rename_custom_icon", { id, name });
  },

  async remove(id: string): Promise<void> {
    await invoke("delete_custom_icon", { id });
  },
};
