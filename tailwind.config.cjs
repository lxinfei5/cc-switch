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
        success: {
          DEFAULT: "hsl(var(--success))",
          foreground: "hsl(var(--success-foreground))",
        },
        warning: {
          DEFAULT: "hsl(var(--warning))",
          foreground: "hsl(var(--warning-foreground))",
        },
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        // Palette ramps re-tuned to the cool, token-harmonized hue family so the
        // existing call sites render coherently with the design tokens. The
        // migration target is semantic tokens (primary/success/warning/destructive/
        // muted); these ramps are kept in sync for the legacy call sites.
        blue: {
          400: "#58A6FF",
          500: "#1473E6",
          600: "#0B5CC7",
        },
        gray: {
          50: "#F7F8FA",
          100: "#EEF0F3",
          200: "#E3E6EB",
          300: "#CFD4DC",
          400: "#9AA1AC",
          500: "#6E747E",
          600: "#565B64",
          700: "#42464E",
          800: "#32353B",
          900: "#26282D",
          950: "#1A1B1F",
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
        1: "var(--shadow-1)",
        2: "var(--shadow-2)",
        3: "var(--shadow-3)",
        sm: "var(--shadow-1)",
        md: "var(--shadow-2)",
        lg: "var(--shadow-3)",
      },
      borderRadius: {
        sm: "calc(var(--radius) - 0.25rem)",
        md: "calc(var(--radius) - 0.125rem)",
        lg: "var(--radius)",
        xl: "calc(var(--radius) + 0.375rem)",
        "2xl": "calc(var(--radius) + 0.625rem)",
      },
      fontFamily: {
        // English → Monaco (no CJK glyphs); Chinese falls through to YaHei.
        // Single source of truth — do not re-declare stacks elsewhere.
        sans: [
          "Monaco",
          '"Microsoft YaHei"',
          '"微软雅黑"',
          '"PingFang SC"',
          '"Hiragino Sans GB"',
          '"Noto Sans CJK SC"',
          "sans-serif",
        ],
        mono: [
          "Monaco",
          '"Microsoft YaHei"',
          '"微软雅黑"',
          "Menlo",
          "Consolas",
          "monospace",
        ],
      },
      // 5-tier type scale (px, not rem) so every page shares the same sizes.
      // caption < xs < sm/base < lg < xl/2xl
      fontSize: {
        caption: ["11px", { lineHeight: "1.4" }],
        xs: ["12px", { lineHeight: "1.45" }],
        sm: ["13px", { lineHeight: "1.5" }],
        base: ["13px", { lineHeight: "1.5" }],
        md: ["14px", { lineHeight: "1.5" }],
        lg: ["16px", { lineHeight: "1.4" }],
        xl: ["18px", { lineHeight: "1.3" }],
        "2xl": ["22px", { lineHeight: "1.2" }],
        // Collapse legacy text-3xl onto the display tier
        "3xl": ["22px", { lineHeight: "1.2" }],
      },
      transitionTimingFunction: {
        expo: "var(--ease-out-expo)",
      },
      animation: {
        "fade-in": "fadeIn 0.2s var(--ease-out-expo)",
        "slide-up": "slideUp 0.2s var(--ease-out-expo)",
        "slide-down": "slideDown 0.2s var(--ease-out-expo)",
        "slide-in-right": "slideInRight 0.2s var(--ease-out-expo)",
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
            transform: "translateY(6px)",
            opacity: "0",
          },
          "100%": {
            transform: "translateY(0)",
            opacity: "1",
          },
        },
        slideDown: {
          "0%": {
            transform: "translateY(-6px)",
            opacity: "0",
          },
          "100%": {
            transform: "translateY(0)",
            opacity: "1",
          },
        },
        slideInRight: {
          "0%": {
            transform: "translateX(6px)",
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
  plugins: [require("tailwindcss-animate")],
};
