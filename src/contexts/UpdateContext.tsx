import {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useRef,
  type ReactNode,
} from "react";
import { checkForUpdate, type UpdateInfo } from "@/lib/updater";
import { extractErrorMessage } from "@/utils/errorUtils";

const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

interface UpdateContextValue {
  hasUpdate: boolean;
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  error: string | null;
  checkUpdate: () => Promise<boolean>;
}

const UpdateContext = createContext<UpdateContextValue | undefined>(undefined);

export function UpdateProvider({ children }: { children: ReactNode }) {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const checkPromise = useRef<Promise<boolean> | null>(null);
  const lastCheckedAt = useRef(0);

  const checkUpdate = useCallback((): Promise<boolean> => {
    if (checkPromise.current) return checkPromise.current;
    setIsChecking(true);
    setError(null);
    lastCheckedAt.current = Date.now();

    checkPromise.current = (async () => {
      try {
        const result = await checkForUpdate({ timeout: 30000 });
        const info = result.status === "available" ? result.info : null;
        setUpdateInfo(info);
        return !!info;
      } catch (err) {
        // 临时断网不应让已经发现的新版本提示消失。
        setError(extractErrorMessage(err));
        throw err;
      } finally {
        setIsChecking(false);
        checkPromise.current = null;
      }
    })();
    return checkPromise.current;
  }, []);

  useEffect(() => {
    const checkInBackground = () => {
      void checkUpdate().catch((err) => {
        console.error("[Updater] Background check failed", err);
      });
    };
    const timer = window.setTimeout(checkInBackground, 1000);
    const interval = window.setInterval(checkInBackground, CHECK_INTERVAL_MS);
    const onVisibilityChange = () => {
      if (
        document.visibilityState === "visible" &&
        Date.now() - lastCheckedAt.current >= CHECK_INTERVAL_MS
      ) {
        checkInBackground();
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.clearTimeout(timer);
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [checkUpdate]);

  return (
    <UpdateContext.Provider
      value={{
        hasUpdate: !!updateInfo,
        updateInfo,
        isChecking,
        error,
        checkUpdate,
      }}
    >
      {children}
    </UpdateContext.Provider>
  );
}

export function useUpdate() {
  const context = useContext(UpdateContext);
  if (!context) {
    throw new Error("useUpdate must be used within UpdateProvider");
  }
  return context;
}
