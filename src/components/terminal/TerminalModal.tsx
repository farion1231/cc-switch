import { useState, useCallback } from "react";
import { X, Maximize2, Minimize2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { Terminal } from "@/components/terminal";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface TerminalModalProps {
  provider: Provider | null;
  appId: AppId;
  isOpen: boolean;
  onClose: () => void;
}

export function TerminalModal({
  provider,
  appId,
  isOpen,
  onClose,
}: TerminalModalProps) {
  const { t } = useTranslation();
  const [isMaximized, setIsMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleError = useCallback((err: string) => {
    setError(err);
  }, []);

  const handleClose = useCallback(() => {
    setError(null);
    onClose();
  }, [onClose]);

  if (!isOpen || !provider) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={handleClose}
      />

      {/* Modal */}
      <div
        className={cn(
          "relative z-10 flex flex-col bg-[#1e1e1e] rounded-lg shadow-2xl overflow-hidden transition-all duration-300",
          isMaximized
            ? "w-[95vw] h-[90vh]"
            : "w-[900px] h-[600px] max-w-[95vw] max-h-[90vh]",
        )}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 bg-[#252526] border-b border-[#333]">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-gray-200">
              {t("provider.openTerminal", "打开终端")} - {provider.name}
            </span>
            <span className="text-xs text-gray-500">({appId})</span>
          </div>

          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 text-gray-400 hover:text-gray-200 hover:bg-[#333]"
              onClick={() => setIsMaximized(!isMaximized)}
            >
              {isMaximized ? (
                <Minimize2 className="h-4 w-4" />
              ) : (
                <Maximize2 className="h-4 w-4" />
              )}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 text-gray-400 hover:text-gray-200 hover:bg-[#333]"
              onClick={handleClose}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Error message */}
        {error && (
          <div className="px-4 py-2 bg-red-500/20 border-b border-red-500/30">
            <p className="text-sm text-red-400">{error}</p>
          </div>
        )}

        {/* Terminal container */}
        <div className="flex-1 overflow-hidden">
          <Terminal
            providerId={provider.id}
            app={appId}
            onError={handleError}
            onClose={handleClose}
          />
        </div>

        {/* Footer */}
        <div className="px-4 py-2 bg-[#252526] border-t border-[#333] text-xs text-gray-500">
          {t("terminal.providerEnvNote", "终端已加载该提供商的环境变量配置")}
        </div>
      </div>
    </div>
  );
}

export default TerminalModal;
