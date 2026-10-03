/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: ["selector", ".dark"],
  theme: {
    extend: {
      // v7 禁用态统一 0.45（tokens.css 的 [aria-disabled] 规则）
      opacity: { 45: "0.45" },
      colors: {
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        // v7 设计 token（见 src/index.css）。纯 var() 颜色不支持 /50 这类透明度修饰。
        app: "var(--bg-app)",
        sidebar: "var(--bg-sidebar)",
        subtle: "var(--bg-subtle)",
        selected: "var(--bg-selected)",
        surface: "var(--bg-card)",
        "border-strong": "var(--border-strong)",
        fg: {
          1: "var(--text-1)",
          2: "var(--text-2)",
          3: "var(--text-3)",
        },
        action: {
          DEFAULT: "var(--action-bg)",
          fg: "var(--action-fg)",
          hover: "var(--action-hover)",
          text: "var(--action-text)",
          soft: "var(--action-soft)",
        },
        inverse: {
          DEFAULT: "var(--inverse-bg)",
          fg: "var(--inverse-fg)",
          hover: "var(--inverse-hover)",
        },
        overlay: "var(--overlay)",
        "control-off": "var(--control-off)",
        direct: {
          DEFAULT: "var(--direct-fill)",
          text: "var(--direct-text)",
          soft: "var(--direct-soft)",
          solid: "var(--direct-solid)",
          on: "var(--direct-on)",
          border: "var(--direct-border)",
        },
        route: {
          DEFAULT: "var(--route-fill)",
          text: "var(--route-text)",
          soft: "var(--route-soft)",
          solid: "var(--route-solid)",
          on: "var(--route-on)",
          border: "var(--route-border)",
        },
        stack: {
          DEFAULT: "var(--stack-fill)",
          text: "var(--stack-text)",
          soft: "var(--stack-soft)",
          solid: "var(--stack-solid)",
          on: "var(--stack-on)",
          border: "var(--stack-border)",
        },
        success: {
          DEFAULT: "var(--success)",
          text: "var(--success-text)",
          soft: "var(--success-soft)",
        },
        warning: {
          DEFAULT: "var(--warning)",
          text: "var(--warning-text)",
          soft: "var(--warning-soft)",
        },
        danger: {
          DEFAULT: "var(--danger)",
          text: "var(--danger-text)",
          soft: "var(--danger-soft)",
        },
        // 会话阅读页 Agent 主题色：text-agent-claude / bg-agent-codex / border-agent-gemini …
        agent: {
          claude: "var(--agent-claude)",
          codex: "var(--agent-codex)",
          gemini: "var(--agent-gemini)",
          opencode: "var(--agent-opencode)",
          pi: "var(--agent-pi)",
          generic: "var(--agent-generic)",
        },
        diff: {
          add: "var(--diff-add-bg)",
          del: "var(--diff-del-bg)",
        },
        chart: {
          1: "var(--chart-1)",
          2: "var(--chart-2)",
          grid: "var(--chart-grid)",
        },
        blue: {
          400: "#409CFF",
          500: "#0A84FF",
          600: "#0060DF",
        },
        gray: {
          50: "#fafafa",
          100: "#f4f4f5",
          200: "#e4e4e7",
          300: "#d4d4d8",
          400: "#a1a1aa",
          500: "#71717a",
          600: "#636366",
          700: "#48484A",
          800: "#3A3A3C",
          900: "#2C2C2E",
          950: "#1C1C1E",
        },
        green: {
          100: "#d1fae5",
          500: "#10b981",
        },
        red: {
          100: "#fee2e2",
          500: "#ef4444",
        },
        amber: {
          100: "#fef3c7",
          500: "#f59e0b",
        },
      },
      boxShadow: {
        sm: "0 1px 2px 0 rgb(0 0 0 / 0.05)",
        md: "0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)",
        lg: "0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)",
        // v7：分段控件凸起、卡片悬停 / 菜单、弹层 / 对话框、抽屉
        "v7-sm": "var(--shadow-sm)",
        "v7-md": "var(--shadow-md)",
        "v7-lg": "var(--shadow-lg)",
      },
      // 只写 border / divide 不带颜色时用主题边框色；不设的话 Tailwind 默认是浅灰 #e4e4e7，深色下成了白线
      borderColor: {
        DEFAULT: "hsl(var(--border))",
      },
      borderRadius: {
        sm: "0.375rem",
        md: "0.5rem",
        lg: "0.75rem",
        xl: "0.875rem",
        // v7：按钮、小控件、侧栏行 / 卡片、分段控件、输入框、弹层、通知条 / 对话框、抽屉
        control: "6px",
        panel: "10px",
        dialog: "14px",
      },
      fontSize: {
        // v7 字体角色（AUTHORING.md「颜色与字体」）
        badge: ["11px", { lineHeight: "16px", fontWeight: "500" }],
        caption: ["12px", { lineHeight: "18px" }],
        body: ["13px", { lineHeight: "20px" }],
        strong: ["14px", { lineHeight: "20px", fontWeight: "500" }],
        section: ["15px", { lineHeight: "22px", fontWeight: "600" }],
        title: ["16px", { lineHeight: "24px", fontWeight: "600" }],
        page: ["18px", { lineHeight: "26px", fontWeight: "600" }],
        metric: ["24px", { lineHeight: "32px", fontWeight: "600" }],
      },
      fontFamily: {
        // 使用与之前版本保持一致的系统字体栈
        sans: [
          "-apple-system",
          "BlinkMacSystemFont",
          '"SF Pro Text"',
          '"PingFang SC"',
          '"Hiragino Sans GB"',
          '"Segoe UI Variable Text"',
          '"Segoe UI"',
          '"Microsoft YaHei UI"',
          '"Microsoft YaHei"',
          "Roboto",
          '"Helvetica Neue"',
          "Arial",
          "system-ui",
          "sans-serif",
        ],
        mono: [
          "ui-monospace",
          "SFMono-Regular",
          '"SF Mono"',
          "Consolas",
          '"Liberation Mono"',
          "Menlo",
          "monospace",
        ],
      },
      animation: {
        "fade-in": "fadeIn 0.5s ease-out",
        "slide-up": "slideUp 0.5s ease-out",
        "slide-down": "slideDown 0.3s ease-out",
        "slide-in-right": "slideInRight 0.3s ease-out",
        "pulse-slow": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
      keyframes: {
        fadeIn: {
          "0%": {
            opacity: "0",
          },
          "100%": {
            opacity: "1",
          },
        },
        slideUp: {
          "0%": {
            transform: "translateY(20px)",
            opacity: "0",
          },
          "100%": {
            transform: "translateY(0)",
            opacity: "1",
          },
        },
        slideDown: {
          "0%": {
            transform: "translateY(-100%)",
            opacity: "0",
          },
          "100%": {
            transform: "translateY(0)",
            opacity: "1",
          },
        },
        slideInRight: {
          "0%": {
            transform: "translateX(100%)",
            opacity: "0",
          },
          "100%": {
            transform: "translateX(0)",
            opacity: "1",
          },
        },
        "accordion-down": {
          from: {
            height: "0",
          },
          to: {
            height: "var(--radix-accordion-content-height)",
          },
        },
        "accordion-up": {
          from: {
            height: "var(--radix-accordion-content-height)",
          },
          to: {
            height: "0",
          },
        },
      },
    },
  },
  // animate-in / fade-in-0 / zoom-in-95 …：Radix 浮层（菜单、弹层、对话框、提示）的进出场动画
  plugins: [require("tailwindcss-animate")],
};
