import React from "react";
import { Provider } from "../types";
import {
  Play,
  Edit3,
  Trash2,
  ExternalLink,
  CheckCircle2,
  Users,
} from "lucide-react";

interface ProviderListProps {
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSwitch: (id: string) => void;
  onDelete: (id: string) => void;
  onEdit: (id: string) => void;
}

const ProviderList: React.FC<ProviderListProps> = ({
  providers,
  currentProviderId,
  onSwitch,
  onDelete,
  onEdit,
}) => {
  // 提取API地址
  const getApiUrl = (provider: Provider): string => {
    try {
      const config = provider.settingsConfig;
      if (config?.env?.ANTHROPIC_BASE_URL) {
        return config.env.ANTHROPIC_BASE_URL;
      }
      return "未设置";
    } catch {
      return "配置错误";
    }
  };

  const handleUrlClick = async (url: string) => {
    try {
      await window.api.openExternal(url);
    } catch (error) {
      console.error("打开链接失败:", error);
    }
  };

  return (
    <div className="space-y-4">
      {Object.values(providers).length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 mx-auto mb-4 bg-[var(--color-bg-tertiary)] rounded-full flex items-center justify-center">
            <Users size={24} className="text-[var(--color-text-tertiary)]" />
          </div>
          <h3 className="text-lg font-medium text-[var(--color-text-primary)] mb-2">
            还没有添加任何供应商
          </h3>
          <p className="text-[var(--color-text-secondary)] text-sm">
            点击右上角的"添加供应商"按钮开始配置您的第一个API供应商
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {Object.values(providers).map((provider) => {
            const isCurrent = provider.id === currentProviderId;
            const apiUrl = getApiUrl(provider);

            return (
              <div
                key={provider.id}
                className={`bg-white rounded-lg border p-4 transition-all duration-200 ${
                  isCurrent
                    ? "border-[var(--color-primary)] ring-1 ring-[var(--color-primary)]/20 bg-[var(--color-primary)]/5"
                    : "border-[var(--color-border)] hover:border-[var(--color-border-hover)] hover:shadow-sm"
                }`}
              >
                <div className="flex items-start justify-between">
                  <div className="flex-1">
                    <div className="flex items-center gap-3 mb-2">
                      <h3 className="font-medium text-[var(--color-text-primary)]">
                        {provider.name}
                      </h3>
                      {isCurrent && (
                        <div className="inline-flex items-center gap-1 px-2 py-1 bg-[var(--color-success)]/10 text-[var(--color-success)] rounded-md text-xs font-medium">
                          <CheckCircle2 size={12} />
                          当前使用
                        </div>
                      )}
                    </div>

                    <div className="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
                      {provider.websiteUrl ? (
                        <button
                          onClick={(e) => {
                            e.preventDefault();
                            handleUrlClick(provider.websiteUrl!);
                          }}
                          className="inline-flex items-center gap-1 hover:text-[var(--color-primary)] transition-colors"
                          title={`访问 ${provider.websiteUrl}`}
                        >
                          <ExternalLink size={14} />
                          {provider.websiteUrl}
                        </button>
                      ) : (
                        <span className="font-mono" title={apiUrl}>
                          {apiUrl}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-2 ml-4">
                    <button
                      onClick={() => onSwitch(provider.id)}
                      disabled={isCurrent}
                      className={`inline-flex items-center gap-1 px-3 py-1.5 text-sm font-medium rounded-md transition-colors ${
                        isCurrent
                          ? "bg-[var(--color-bg-tertiary)] text-[var(--color-text-tertiary)] cursor-not-allowed"
                          : "bg-[var(--color-primary)] text-white hover:bg-[var(--color-primary-hover)]"
                      }`}
                    >
                      <Play size={14} />
                      {isCurrent ? "使用中" : "启用"}
                    </button>

                    <button
                      onClick={() => onEdit(provider.id)}
                      className="p-1.5 text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] hover:bg-[var(--color-bg-tertiary)] rounded-md transition-colors"
                      title="编辑供应商"
                    >
                      <Edit3 size={16} />
                    </button>

                    <button
                      onClick={() => onDelete(provider.id)}
                      disabled={isCurrent}
                      className={`p-1.5 rounded-md transition-colors ${
                        isCurrent
                          ? "text-[var(--color-text-tertiary)] cursor-not-allowed"
                          : "text-[var(--color-text-secondary)] hover:text-[var(--color-error)] hover:bg-[var(--color-error-light)]"
                      }`}
                      title="删除供应商"
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default ProviderList;
