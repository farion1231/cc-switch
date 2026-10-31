import { render, screen, act, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { McpHoverCard } from "@/components/mcp/McpHoverCard";
import type { McpServer } from "@/types";
import type { McpAppId } from "@/config/appConfig";

describe("McpHoverCard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  const appIds: McpAppId[] = ["claude", "codex", "gemini"];

  const serverStdio: McpServer = {
    id: "postgres",
    name: "PostgreSQL Database",
    description: "Read-only SQL tool for database analysis",
    tags: ["database", "sql", "analysis"],
    homepage: "https://example.com/postgres",
    docs: "https://example.com/postgres/docs",
    server: {
      type: "stdio",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-postgres"],
      cwd: "/projects/db",
      env: {
        DATABASE_URL: "secret-url",
        API_KEY: "secret-key",
      },
    },
    apps: {
      claude: true,
      codex: true,
      gemini: false,
    } as McpServer["apps"],
  };

  const serverHttp: McpServer = {
    id: "remote-api",
    name: "remote-api",
    server: {
      type: "http",
      url: "https://api.example.com/mcp",
      headers: {
        Authorization: "Bearer secret-token",
      },
    },
    apps: {
      claude: false,
      codex: false,
      gemini: false,
    } as McpServer["apps"],
  };

  it("renders trigger children and opens hover card after delay", () => {
    render(
      <McpHoverCard id="postgres" server={serverStdio} appIds={appIds}>
        <span>PostgreSQL Trigger</span>
      </McpHoverCard>,
    );

    expect(screen.getByText("PostgreSQL Trigger")).toBeInTheDocument();
    expect(
      screen.queryByTestId("mcp-hover-card-postgres"),
    ).not.toBeInTheDocument();

    const trigger = screen.getByTestId("mcp-hover-trigger-postgres");
    fireEvent.pointerEnter(trigger);

    // Before 300ms delay, card is not yet open
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(
      screen.queryByTestId("mcp-hover-card-postgres"),
    ).not.toBeInTheDocument();

    // After 300ms delay, card opens
    act(() => {
      vi.advanceTimersByTime(150);
    });
    expect(screen.getByTestId("mcp-hover-card-postgres")).toBeInTheDocument();
  });

  it("renders full configuration details, masked env variables, and enabled clients", () => {
    render(
      <McpHoverCard id="postgres" server={serverStdio} appIds={appIds}>
        <span>PostgreSQL Trigger</span>
      </McpHoverCard>,
    );

    const trigger = screen.getByTestId("mcp-hover-trigger-postgres");
    fireEvent.pointerEnter(trigger);
    act(() => {
      vi.advanceTimersByTime(350);
    });

    const card = screen.getByTestId("mcp-hover-card-postgres");
    expect(card).toBeInTheDocument();

    // Title and ID
    expect(screen.getByText("PostgreSQL Database")).toBeInTheDocument();
    expect(screen.getByText("id: postgres")).toBeInTheDocument();

    // Transport badge
    expect(screen.getByText("stdio")).toBeInTheDocument();

    // Description
    expect(
      screen.getByText("Read-only SQL tool for database analysis"),
    ).toBeInTheDocument();

    // Tags
    expect(screen.getByText("#database")).toBeInTheDocument();
    expect(screen.getByText("#sql")).toBeInTheDocument();
    expect(screen.getByText("#analysis")).toBeInTheDocument();

    // Command, args and cwd
    expect(
      screen.getByText("npx -y @modelcontextprotocol/server-postgres"),
    ).toBeInTheDocument();
    expect(screen.getByText("cwd: /projects/db")).toBeInTheDocument();

    // Masked environment variables
    expect(screen.getByText("DATABASE_URL=••••••••")).toBeInTheDocument();
    expect(screen.getByText("API_KEY=••••••••")).toBeInTheDocument();
    expect(screen.queryByText("secret-url")).not.toBeInTheDocument();
    expect(screen.queryByText("secret-key")).not.toBeInTheDocument();

    // Client matrix summary (2/3 enabled)
    expect(screen.getByText("2/3")).toBeInTheDocument();
  });

  it("renders HTTP url and masked headers for remote server", () => {
    render(
      <McpHoverCard id="remote-api" server={serverHttp} appIds={appIds}>
        <span>Remote Trigger</span>
      </McpHoverCard>,
    );

    const trigger = screen.getByTestId("mcp-hover-trigger-remote-api");
    fireEvent.pointerEnter(trigger);
    act(() => {
      vi.advanceTimersByTime(350);
    });

    expect(screen.getByText("https://api.example.com/mcp")).toBeInTheDocument();
    expect(screen.getByText("Authorization: ••••••••")).toBeInTheDocument();
    expect(screen.queryByText("Bearer secret-token")).not.toBeInTheDocument();
  });

  it("closes after mouse leaves trigger", () => {
    render(
      <McpHoverCard id="postgres" server={serverStdio} appIds={appIds}>
        <span>PostgreSQL Trigger</span>
      </McpHoverCard>,
    );

    const trigger = screen.getByTestId("mcp-hover-trigger-postgres");
    fireEvent.pointerEnter(trigger);
    act(() => {
      vi.advanceTimersByTime(350);
    });
    expect(screen.getByTestId("mcp-hover-card-postgres")).toBeInTheDocument();

    fireEvent.pointerLeave(trigger);
    act(() => {
      vi.advanceTimersByTime(250);
    });
    expect(
      screen.queryByTestId("mcp-hover-card-postgres"),
    ).not.toBeInTheDocument();
  });

  it("calls onOpenHomepage and onOpenDocs when clicking link buttons", () => {
    const onOpenDocs = vi.fn();
    const onOpenHomepage = vi.fn();

    render(
      <McpHoverCard
        id="postgres"
        server={serverStdio}
        appIds={appIds}
        onOpenDocs={onOpenDocs}
        onOpenHomepage={onOpenHomepage}
      >
        <span>PostgreSQL Trigger</span>
      </McpHoverCard>,
    );

    const trigger = screen.getByTestId("mcp-hover-trigger-postgres");
    fireEvent.pointerEnter(trigger);
    act(() => {
      vi.advanceTimersByTime(350);
    });

    const homeBtn = screen.getByRole("button", {
      name: "mcpPage.drawer.homepage",
    });
    const docsBtn = screen.getByRole("button", { name: "mcpPage.openDocs" });

    fireEvent.click(homeBtn);
    expect(onOpenHomepage).toHaveBeenCalledWith("https://example.com/postgres");

    fireEvent.click(docsBtn);
    expect(onOpenDocs).toHaveBeenCalled();
  });

  it("closes immediately when trigger is clicked", () => {
    render(
      <McpHoverCard id="postgres" server={serverStdio} appIds={appIds}>
        <span>PostgreSQL Trigger</span>
      </McpHoverCard>,
    );

    const trigger = screen.getByTestId("mcp-hover-trigger-postgres");
    fireEvent.pointerEnter(trigger);
    act(() => {
      vi.advanceTimersByTime(350);
    });
    expect(screen.getByTestId("mcp-hover-card-postgres")).toBeInTheDocument();

    fireEvent.click(trigger);
    expect(
      screen.queryByTestId("mcp-hover-card-postgres"),
    ).not.toBeInTheDocument();
  });
});
