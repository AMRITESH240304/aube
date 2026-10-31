export const THEMES = [
  {
    id: "haunted",
    name: "Haunted",
    note: "Moonlit violet and drifting mist",
    bg: "radial-gradient(circle at 85% 0%, #3a2a5c, #0c0815 70%)",
    fg: "#e9e2f4",
    accent: "linear-gradient(180deg, #d9c6ff, #a985ff)",
    font: '"Iowan Old Style", serif',
    weight: 600,
    radius: "9999px",
  },
  {
    id: "terminal",
    name: "Terminal",
    note: "Hairline grid and mono labels",
    bg: "linear-gradient(to right, #ffffff10 1px, transparent 1px) 0 0 / 18px 18px, linear-gradient(#ffffff10 1px, transparent 1px) 0 0 / 18px 18px, #09090b",
    fg: "#ededf0",
    accent: "#b9a6ff",
    font: "ui-monospace, monospace",
    weight: 400,
    radius: "0",
  },
  {
    id: "ship",
    name: "Ship",
    note: "Warm black, cream and lime",
    bg: "#121210",
    fg: "#f2efe4",
    accent: "#eef56b",
    font: "system-ui",
    weight: 800,
    radius: "9999px",
  },
  {
    id: "glass",
    name: "Glass",
    note: "Your desktop shows through",
    bg: "linear-gradient(160deg, #4d6fb8, #7b4a9e 55%, #10141c)",
    fg: "#ffffff",
    accent: "linear-gradient(180deg, #5aaeff, #0a72ff)",
    font: "system-ui",
    weight: 600,
    radius: "9999px",
  },
] as const;

export type Theme = (typeof THEMES)[number]["id"];

class ThemeState {
  current = $state<Theme>((document.documentElement.dataset.theme as Theme) ?? "haunted");

  set(theme: Theme) {
    this.current = theme;
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("theme", theme);
    } catch {}
  }
}

export const theme = new ThemeState();
