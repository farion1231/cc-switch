import { invoke } from "@tauri-apps/api/core";

/**
 * 供应商自定义图标资产 API（issue #8117）。
 * 资产由 Rust 端存放在 `<app_config>/assets/icons/`，前端只持有资产键。
 */

export interface SavedProviderIcon {
  /** 存入 provider.icon（iconSource = "local"）的资产键 */
  key: string;
  width: number;
  height: number;
  byteSize: number;
  mimeType: string;
}

/** 打开系统文件选择框，返回本地图片路径（用户取消返回 null）。 */
export async function pickProviderIconFile(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "svg"] },
    ],
  });
  if (typeof selected === "string") return selected;
  return null;
}

/** 校验并复制所选图片进图标资产目录，返回资产元数据。 */
export function saveProviderIcon(
  providerId: string,
  sourcePath: string,
): Promise<SavedProviderIcon> {
  return invoke("save_provider_icon", { providerId, sourcePath });
}

/** 读取资产为 data: URL；文件缺失返回 null（调用方静默回退内建图标链）。 */
export function readProviderIcon(key: string): Promise<string | null> {
  return invoke<string | null>("read_provider_icon", { key });
}

/** 删除一个供应商的全部图标资产（图标重置 / 供应商删除时调用）。 */
export function deleteProviderIcon(providerId: string): Promise<boolean> {
  return invoke("delete_provider_icon", { providerId });
}
