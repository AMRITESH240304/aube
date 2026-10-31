export const THEMES = [
  {
    id: "dusk",
    name: "Dusk",
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
] as const;

export type Theme = (typeof THEMES)[number]["id"];

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

class ThemeState {
  current = $state<Theme>((document.documentElement.dataset.theme as Theme) ?? "dusk");

  set(theme: Theme) {
    this.current = theme;
    document.documentElement.dataset.theme = theme;
    remember("theme", theme);
  }
}

export const theme = new ThemeState();
