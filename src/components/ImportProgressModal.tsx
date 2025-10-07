import { useEffect } from "react";
import { CheckCircle, Loader2, AlertCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ImportProgressModalProps {
  status: 'importing' | 'success' | 'error';
  message?: string;
  backupId?: string;
  onComplete?: () => void;
  onSuccess?: () => void;
}

export function ImportProgressModal({
  status,
  message,
  backupId,
  onComplete,
  onSuccess
}: ImportProgressModalProps) {
  const { t } = useTranslation();

  useEffect(() => {
    if (status === 'success') {
      console.log('[ImportProgressModal] Success detected, starting 2 second countdown');
      // 成功后等待2秒自动关闭并刷新数据
      const timer = setTimeout(() => {
        console.log('[ImportProgressModal] 2 seconds elapsed, calling callbacks...');
        if (onSuccess) {
          onSuccess();
        }
        if (onComplete) {
          onComplete();
        }
      }, 2000);

      return () => {
        console.log('[ImportProgressModal] Cleanup timer');
        clearTimeout(timer);
      };
    }
  }, [status, onComplete, onSuccess]);

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center">
      <div className="absolute inset-0 bg-black/50 dark:bg-black/70 backdrop-blur-sm" />

      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-2xl p-8 max-w-md w-full mx-4">
        <div className="flex flex-col items-center text-center">
          {status === 'importing' && (
            <>
              <Loader2 className="w-12 h-12 text-blue-500 animate-spin mb-4" />
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2">
                {t("settings.importing")}
              </h3>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                {t("common.loading")}
              </p>
            </>
          )}

          {status === 'success' && (
            <>
              <CheckCircle className="w-12 h-12 text-green-500 mb-4" />
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2">
                {t("settings.importSuccess")}
              </h3>
              {backupId && (
                <p className="text-sm text-gray-600 dark:text-gray-400 mb-2">
                  {t("settings.backupId")}: {backupId}
                </p>
              )}
              <p className="text-sm text-gray-600 dark:text-gray-400">
                {t("settings.autoReload")}
              </p>
            </>
          )}

          {status === 'error' && (
            <>
              <AlertCircle className="w-12 h-12 text-red-500 mb-4" />
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-2">
                {t("settings.importFailed")}
              </h3>
              <p className="text-sm text-gray-600 dark:text-gray-400">
                {message || t("settings.configCorrupted")}
              </p>
              <button
                onClick={() => {
                  if (onComplete) {
                    onComplete();
                  }
                }}
                className="mt-4 px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors"
              >
                {t("common.close")}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}