// Light or dark: the setting, or Windows' choice when it's "system". Written onto <html data-theme>,
// where styles.css picks its colors, and shared with canvas drawing through `ThemeContext`.

import { createContext, useContext, useEffect, useState } from "react";
import type { Theme } from "./api";

export type Shade = "light" | "dark";

const dev = new URLSearchParams(location.search).get("theme");

function systemShade(): Shade {
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Resolves the theme setting and applies it to the page. */
export function useApplyTheme(setting: Theme | undefined): Shade {
  const [system, setSystem] = useState<Shade>(systemShade);

  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setSystem(systemShade());
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  // `?theme=dark` wins in the dev mock, so screenshots can show both.
  const shade: Shade = dev === "light" || dev === "dark" ? dev : setting === "light" || setting === "dark" ? setting : system;

  useEffect(() => {
    document.documentElement.dataset.theme = shade;
    document.documentElement.style.colorScheme = shade;
  }, [shade]);

  return shade;
}

export const ThemeContext = createContext<Shade>("light");
export const useShade = () => useContext(ThemeContext);
