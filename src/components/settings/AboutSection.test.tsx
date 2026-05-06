import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { AboutSection } from "./AboutSection";

const toolVersionsMock = vi.fn();
const diagnoseEnvironmentMock = vi.fn();
const installToolMock = vi.fn();
const fixEnvironmentMock = vi.fn();
const getVersionMock = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) => {
      if (key === "doctor.issuesFound" && vars?.count) {
        return `issues:${vars.count}`;
      }
      if (key === "doctor.installSuccess" && vars?.tool) {
        return `installSuccess:${vars.tool}`;
      }
      if (key === "doctor.fixSuccess" && vars?.count) {
        return `fixSuccess:${vars.count}`;
      }
      if (key === "doctor.fixFailed" && vars?.error) {
        return `fixFailed:${vars.error}`;
      }
      if (key === "doctor.installFailed" && vars?.error) {
        return `installFailed:${vars.error}`;
      }
      if (key === "settings.updateTo" && vars?.version) {
        return `updateTo:${vars.version}`;
      }
      return key;
    },
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => getVersionMock(),
}));

vi.mock("@/lib/platform", () => ({
  isWindows: () => false,
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getToolVersions: (...args: unknown[]) => toolVersionsMock(...args),
    openExternal: vi.fn(),
    checkUpdates: vi.fn(),
  },
}));

vi.mock("@/lib/api/doctor", () => ({
  doctorApi: {
    diagnoseEnvironment: (...args: unknown[]) => diagnoseEnvironmentMock(...args),
    installTool: (...args: unknown[]) => installToolMock(...args),
    fixEnvironment: (...args: unknown[]) => fixEnvironmentMock(...args),
  },
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: false,
    updateInfo: null,
    updateHandle: null,
    checkUpdate: vi.fn().mockResolvedValue(false),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));

vi.mock("@/lib/updater", () => ({
  relaunchApp: vi.fn(),
}));

vi.mock("framer-motion", async () => {
  const React = await import("react");
  const MotionDiv = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
    (props, ref) => React.createElement("div", { ...props, ref }, props.children),
  );
  const MotionSection = React.forwardRef<HTMLElement, React.HTMLAttributes<HTMLElement>>(
    (props, ref) => React.createElement("section", { ...props, ref }, props.children),
  );

  return {
    motion: {
      div: MotionDiv,
      section: MotionSection,
    },
  };
});

vi.mock("@/assets/icons/app-icon.png", () => ({
  default: "app-icon-mock",
}));

describe("AboutSection environment doctor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getVersionMock.mockResolvedValue("3.14.1");
    toolVersionsMock.mockResolvedValue([
      {
        name: "claude",
        version: "2.1.131",
        latest_version: "2.1.131",
        error: null,
        env_type: "macos",
        wsl_distro: null,
      },
    ]);
  });

  it("renders one-click install when diagnosis needs install", async () => {
    diagnoseEnvironmentMock.mockResolvedValue({
      overall_status: "NeedsInstall",
      issues: [
        {
          id: "nodejs_missing",
          severity: "Critical",
          category: "NodeJsMissing",
          title: "Node.js 环境问题",
          description: "Node.js 未安装",
          auto_fixable: false,
          fix_action: { type: "InstallNodeJs" },
        },
      ],
      tools_status: {},
    });

    render(<AboutSection isPortable={false} />);

    await waitFor(() => {
      expect(screen.getByText("doctor.environmentStatus")).toBeInTheDocument();
    });

    expect(
      screen.getByRole("button", { name: "doctor.oneClickInstall" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "doctor.oneClickFix" }),
    ).not.toBeInTheDocument();
  });

  it("renders one-click fix when diagnosis has auto-fixable repair issues", async () => {
    diagnoseEnvironmentMock.mockResolvedValue({
      overall_status: "NeedsRepair",
      issues: [
        {
          id: "env_conflict_OPENAI_API_KEY",
          severity: "High",
          category: "EnvConflict",
          title: "环境变量冲突",
          description: "检测到冲突",
          auto_fixable: true,
          fix_action: {
            type: "RemoveEnvVar",
            var_name: "OPENAI_API_KEY",
            source: "~/.zshrc",
          },
        },
      ],
      tools_status: {},
    });

    render(<AboutSection isPortable={false} />);

    await waitFor(() => {
      expect(screen.getByText("doctor.environmentStatus")).toBeInTheDocument();
    });

    expect(
      screen.getByRole("button", { name: "doctor.oneClickFix" }),
    ).toBeInTheDocument();
  });
});
