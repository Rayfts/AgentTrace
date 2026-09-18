import { useEffect, useState } from "react";

type ThemePreference = "system" | "dark" | "light";
type ResolvedTheme = "dark" | "light";

const STORAGE_KEY = "agenttrace.theme";

function storedPreference(): ThemePreference {
  const value = localStorage.getItem(STORAGE_KEY);
  return value === "dark" || value === "light" || value === "system" ? value : "system";
}

function resolvedTheme(preference: ThemePreference, media: MediaQueryList): ResolvedTheme {
  if (preference === "system") return media.matches ? "dark" : "light";
  return preference;
}

export function ThemeControl() {
  const [preference, setPreference] = useState<ThemePreference>(storedPreference);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme = resolvedTheme(preference, media);
      document.documentElement.style.colorScheme = resolvedTheme(preference, media);
    };
    apply();
    localStorage.setItem(STORAGE_KEY, preference);
    if (preference !== "system") return;
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [preference]);

  return (
    <label className="theme-control" title="Desktop theme">
      <span>Theme</span>
      <select value={preference} onChange={(event) => setPreference(event.target.value as ThemePreference)}>
        <option value="system">System</option>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
      </select>
    </label>
  );
}
