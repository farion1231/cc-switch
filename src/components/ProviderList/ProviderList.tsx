import React from "react";
import { Provider } from "../../types";
import "./ProviderList.css";

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
    <div className="provider-list">
      {Object.values(providers).length === 0 ? (
        <div className="empty-state">
          <p>还没有添加任何供应商</p>
          <p>点击右上角的"添加供应商"按钮开始</p>
        </div>
      ) : (
        <div className="provider-items">
          {Object.values(providers).map((provider) => {
            const isCurrent = provider.id === currentProviderId;

            return (
              <div
                key={provider.id}
                className={`provider-item ${isCurrent ? "current" : ""}`}
              >
                <div className="provider-info">
                  <div className="provider-name">
                    <span>{provider.name}</span>
                    {isCurrent && (
                      <span className="current-badge">当前使用</span>
                    )}
                  </div>
                  <div className="provider-url">
                    {provider.websiteUrl ? (
                      <a
                        href="#"
                        onClick={(e) => {
                          e.preventDefault();
                          handleUrlClick(provider.websiteUrl!);
                        }}
                        className="url-link"
                        title={`访问 ${provider.websiteUrl}`}
                      >
                        {provider.websiteUrl}
                      </a>
                    ) : (
                      <span className="api-url" title={getApiUrl(provider)}>
                        {getApiUrl(provider)}
                      </span>
                    )}
                  </div>
                </div>

                <div className="provider-actions">
                  <button
                    className="enable-btn"
                    onClick={() => onSwitch(provider.id)}
                    disabled={isCurrent}
                  >
                    启用
                  </button>
                  <button
                    className="edit-btn"
                    onClick={() => onEdit(provider.id)}
                  >
                    编辑
                  </button>
                  <button
                    className="delete-btn"
                    onClick={() => onDelete(provider.id)}
                    disabled={isCurrent}
                  >
                    删除
                  </button>
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
