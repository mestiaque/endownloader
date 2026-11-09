/** @type {import('tailwindcss').Config} */
const v = (name) => `rgb(var(--${name}) / <alpha-value>)`;
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: ["class"],
  theme: {
    extend: {
      colors: {
        bg: v("bg"),
        panel: v("panel"),
        raised: v("raised"),
        line: v("line"),
        fg: v("fg"),
        muted: v("muted"),
        faint: v("faint"),
        accent: v("accent"),
        "accent-fg": v("accent-fg"),
        danger: v("danger"),
        success: v("success"),
        // Viewer foreground; switches with the viewer style (dark / light / glass).
        vfg: v("v-fg"),
      },
      fontFamily: {
        sans: ["Inter", "Segoe UI Variable", "Segoe UI", "Ubuntu", "Cantarell", "system-ui", "sans-serif"],
        mono: ["JetBrains Mono", "Cascadia Mono", "Ubuntu Mono", "monospace"],
      },
      boxShadow: {
        pop: "0 12px 40px -8px rgb(0 0 0 / 0.45), 0 0 0 1px rgb(var(--line) / 0.9)",
      },
      keyframes: {
        "fade-in": { from: { opacity: 0 }, to: { opacity: 1 } },
        "scale-in": { from: { opacity: 0, transform: "scale(.97)" }, to: { opacity: 1, transform: "scale(1)" } },
        "slide-up": { from: { opacity: 0, transform: "translateY(6px)" }, to: { opacity: 1, transform: "translateY(0)" } },
        shimmer: { "100%": { transform: "translateX(100%)" } },
      },
      animation: {
        "fade-in": "fade-in .15s ease-out",
        "scale-in": "scale-in .16s cubic-bezier(.2,.8,.2,1)",
        "slide-up": "slide-up .18s cubic-bezier(.2,.8,.2,1)",
      },
    },
  },
  plugins: [],
};
