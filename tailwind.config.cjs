/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: ["selector", ".dark"],
  theme: {
    extend: {
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
        success: "hsl(var(--success))",
        warning: "hsl(var(--warning))",
        sidebar: {
          DEFAULT: "hsl(var(--sidebar))",
          hover: "hsl(var(--sidebar-hover))",
          active: "hsl(var(--sidebar-active))",
          "active-foreground": "hsl(var(--sidebar-active-foreground))",
          foreground: "hsl(var(--sidebar-foreground))",
          muted: "hsl(var(--sidebar-muted))",
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
        sm: "0 1px 2px 0 rgb(0 0 0 / 0.04)",
        md: "0 2px 4px -1px rgb(0 0 0 / 0.06), 0 1px 2px -1px rgb(0 0 0 / 0.04)",
        lg: "0 4px 12px -2px rgb(0 0 0 / 0.08), 0 2px 6px -2px rgb(0 0 0 / 0.04)",
        xl: "0 8px 24px -4px rgb(0 0 0 / 0.1), 0 4px 8px -2px rgb(0 0 0 / 0.04)",
      },
      borderRadius: {
        sm: "0.375rem",
        md: "0.5rem",
        lg: "0.75rem",
        xl: "0.875rem",
      },
      fontFamily: {
        sans: [
          "-apple-system",
          '"SF Pro Text"',
          '"Segoe UI Variable Text"',
          '"Segoe UI"',
          '"PingFang SC"',
          '"Microsoft YaHei UI"',
          '"Helvetica Neue"',
          "sans-serif",
        ],
        display: [
          '"Bricolage Grotesque Variable"',
          '"Bricolage Grotesque"',
          '"PingFang SC"',
          '"Microsoft YaHei UI"',
          '"Helvetica Neue"',
          "sans-serif",
        ],
        mono: [
          '"IBM Plex Mono"',
          "ui-monospace",
          '"SF Mono"',
          "Consolas",
          '"PingFang SC"',
          '"Microsoft YaHei UI"',
          "monospace",
        ],
      },
      animation: {
        "fade-in": "fadeIn 0.3s cubic-bezier(0.25, 0.1, 0.25, 1)",
        "slide-up": "slideUp 0.3s cubic-bezier(0.25, 0.1, 0.25, 1)",
        "slide-down": "slideDown 0.2s cubic-bezier(0.25, 0.1, 0.25, 1)",
        "slide-in-right": "slideInRight 0.25s cubic-bezier(0.25, 0.1, 0.25, 1)",
        "scale-in": "scaleIn 0.2s cubic-bezier(0.25, 0.1, 0.25, 1)",
        "pulse-slow": "pulse 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
      keyframes: {
        fadeIn: {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
        slideUp: {
          "0%": { transform: "translateY(12px)", opacity: "0" },
          "100%": { transform: "translateY(0)", opacity: "1" },
        },
        slideDown: {
          "0%": { transform: "translateY(-8px)", opacity: "0" },
          "100%": { transform: "translateY(0)", opacity: "1" },
        },
        slideInRight: {
          "0%": { transform: "translateX(16px)", opacity: "0" },
          "100%": { transform: "translateX(0)", opacity: "1" },
        },
        scaleIn: {
          "0%": { transform: "scale(0.96)", opacity: "0" },
          "100%": { transform: "scale(1)", opacity: "1" },
        },
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
      },
    },
  },
  plugins: [],
};

