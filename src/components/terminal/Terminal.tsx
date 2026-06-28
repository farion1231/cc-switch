import { useEffect, useRef, useCallback } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { connectTerminalWebSocket } from "@/lib/api/web-client";

interface TerminalProps {
  providerId: string;
  app: string;
  onError?: (error: string) => void;
  onClose?: () => void;
}

export function Terminal({ providerId, app, onError, onClose }: TerminalProps) {
  const terminalRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<ReturnType<typeof connectTerminalWebSocket> | null>(
    null,
  );
  const encoderRef = useRef<TextEncoder>(new TextEncoder());

  const handleData = useCallback((data: string) => {
    if (wsRef.current) {
      const bytes = encoderRef.current.encode(data);
      wsRef.current.send(bytes);
    }
  }, []);

  const handleResize = useCallback(() => {
    if (fitAddonRef.current && xtermRef.current && wsRef.current) {
      fitAddonRef.current.fit();
      const { cols, rows } = xtermRef.current;
      wsRef.current.resize(cols, rows);
    }
  }, []);

  useEffect(() => {
    if (!terminalRef.current) return;

    // Create terminal instance
    const term = new XTerm({
      cursorBlink: true,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      fontSize: 14,
      theme: {
        background: "#1e1e1e",
        foreground: "#d4d4d4",
        cursor: "#d4d4d4",
        selectionBackground: "#264f78",
        black: "#000000",
        red: "#cd3131",
        green: "#0dbc79",
        yellow: "#e5e510",
        blue: "#2472c8",
        magenta: "#bc3fbc",
        cyan: "#11a8cd",
        white: "#e5e5e5",
        brightBlack: "#666666",
        brightRed: "#f14c4c",
        brightGreen: "#23d18b",
        brightYellow: "#f5f543",
        brightBlue: "#3b8eea",
        brightMagenta: "#d670d6",
        brightCyan: "#29b8db",
        brightWhite: "#e5e5e5",
      },
    });

    // Create fit addon
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Open terminal in container
    term.open(terminalRef.current);
    fitAddon.fit();

    // Store refs
    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // Connect to WebSocket
    const ws = connectTerminalWebSocket(
      providerId,
      app,
      (data: Uint8Array) => {
        const decoder = new TextDecoder();
        const text = decoder.decode(data);
        term.write(text);
      },
      () => {
        term.writeln(
          "\r\n\x1b[32mTerminal connected. Provider environment loaded.\x1b[0m\r\n",
        );
        handleResize();
      },
      (error: string) => {
        term.writeln(`\r\n\x1b[31mError: ${error}\x1b[0m\r\n`);
        onError?.(error);
      },
      () => {
        // Close
        term.writeln("\r\n\x1b[33mTerminal connection closed.\x1b[0m\r\n");
        onClose?.();
      },
    );

    wsRef.current = ws;

    // Handle terminal input
    term.onData(handleData);

    // Handle window resize
    const handleWindowResize = () => {
      handleResize();
    };

    window.addEventListener("resize", handleWindowResize);

    // Cleanup
    return () => {
      window.removeEventListener("resize", handleWindowResize);
      ws.close();
      term.dispose();
      xtermRef.current = null;
      fitAddonRef.current = null;
      wsRef.current = null;
    };
  }, [providerId, app, handleData, handleResize, onError, onClose]);

  return (
    <div
      ref={terminalRef}
      className="h-full w-full bg-[#1e1e1e] p-2"
      style={{
        fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      }}
    />
  );
}

export default Terminal;
