import { useState, type FormEvent } from "react";
import { Eye, EyeOff, KeyRound, Loader2 } from "lucide-react";
import { tr, type MobileLang } from "../i18n";

interface SignInScreenProps {
  lang: MobileLang;
  error: string | null;
  submitting: boolean;
  onSubmit: (token: string) => void;
}

export function SignInScreen({
  lang,
  error,
  submitting,
  onSubmit,
}: SignInScreenProps) {
  const [token, setToken] = useState("");
  const [reveal, setReveal] = useState(false);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    const trimmed = token.trim();
    if (!trimmed || submitting) return;
    onSubmit(trimmed);
  };

  return (
    <div className="flex min-h-[100dvh] flex-col items-center justify-center bg-[#f5f5f7] px-5 py-10 dark:bg-[#18181b]">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex items-center justify-center gap-2.5">
          <img
            src="/icons/icon-256.png"
            alt="CC Switch"
            className="h-9 w-9 rounded-xl shadow-sm"
          />
          <span className="text-lg font-semibold tracking-tight text-zinc-900 dark:text-zinc-100">
            CC Switch
          </span>
        </div>

        <form
          onSubmit={handleSubmit}
          className="rounded-2xl border border-zinc-200/80 bg-white p-5 shadow-sm dark:border-zinc-800 dark:bg-[#232328]"
        >
          <div className="mb-4 flex items-start gap-3">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-orange-500/10 text-orange-500">
              <KeyRound className="h-5 w-5" />
            </div>
            <div>
              <h1 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                {tr(lang, "signInTitle")}
              </h1>
              <p className="mt-1 text-xs leading-relaxed text-zinc-500 dark:text-zinc-400">
                {tr(lang, "signInDesc")}
              </p>
            </div>
          </div>

          <label
            htmlFor="lan-token-input"
            className="mb-1.5 block text-xs font-medium text-zinc-700 dark:text-zinc-300"
          >
            {tr(lang, "tokenLabel")}
          </label>
          <div className="relative">
            <input
              id="lan-token-input"
              type={reveal ? "text" : "password"}
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder={tr(lang, "tokenPlaceholder")}
              autoComplete="current-password"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              required
              className="h-12 w-full rounded-xl border border-zinc-300 bg-zinc-50 pl-3.5 pr-11 font-mono text-[16px] text-zinc-900 outline-none transition focus:border-orange-500 focus:ring-2 focus:ring-orange-500/20 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100"
            />
            <button
              type="button"
              onClick={() => setReveal((v) => !v)}
              aria-label={reveal ? "Hide token" : "Show token"}
              className="absolute right-1.5 top-1/2 flex h-9 w-9 -translate-y-1/2 items-center justify-center rounded-lg text-zinc-500 hover:bg-zinc-200/70 dark:hover:bg-zinc-800"
            >
              {reveal ? (
                <EyeOff className="h-4 w-4" />
              ) : (
                <Eye className="h-4 w-4" />
              )}
            </button>
          </div>

          {error && (
            <p
              role="alert"
              className="mt-3 rounded-xl bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400"
            >
              {error}
            </p>
          )}

          <button
            type="submit"
            disabled={submitting || !token.trim()}
            className="mt-4 flex h-12 w-full items-center justify-center gap-2 rounded-xl bg-[#f97316] text-sm font-semibold text-white transition active:scale-[0.98] disabled:opacity-50"
          >
            {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
            {submitting ? tr(lang, "signingIn") : tr(lang, "signInBtn")}
          </button>
        </form>
      </div>
    </div>
  );
}
