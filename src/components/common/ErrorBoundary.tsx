import React, { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Alert, AlertTitle, AlertDescription } from "@/components/ui/alert";
import { TriangleAlert } from "lucide-react";

interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: ReactNode;
  onError?: (error: Error, errorInfo: React.ErrorInfo) => void;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error?: Error;
}

function ErrorFallback({ error }: { error?: Error }) {
  const { t } = useTranslation();
  return (
    <Alert variant="destructive" className="m-4">
      <TriangleAlert className="h-4 w-4" />
      <AlertTitle>{t("errorBoundary.title")}</AlertTitle>
      <AlertDescription>
        {t("errorBoundary.description")}
        {process.env.NODE_ENV === "development" && error && (
          <pre className="mt-2 text-xs whitespace-pre-wrap">
            {error.message}
          </pre>
        )}
      </AlertDescription>
    </Alert>
  );
}

export class ErrorBoundary extends React.Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error("[ErrorBoundary] Caught error:", error, errorInfo);
    this.props.onError?.(error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return this.props.fallback ?? <ErrorFallback error={this.state.error} />;
    }
    return this.props.children;
  }
}
