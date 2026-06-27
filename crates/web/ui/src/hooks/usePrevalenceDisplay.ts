import { createContext, useCallback, useContext, useState } from "react";

/** Display modes for prevalence badges. */
export type PrevalenceDisplayMode = "fraction" | "percent";

const CYCLE_ORDER: PrevalenceDisplayMode[] = ["fraction", "percent"];

export interface PrevalenceDisplayContextValue {
  mode: PrevalenceDisplayMode;
  cycle: () => void;
}

export const PrevalenceDisplayContext =
  createContext<PrevalenceDisplayContextValue>({
    mode: "fraction",
    cycle: () => {},
  });

/** Provide prevalence display state to the component tree. */
export function usePrevalenceDisplayState(): PrevalenceDisplayContextValue {
  const [mode, setMode] = useState<PrevalenceDisplayMode>("fraction");
  const cycle = useCallback(() => {
    setMode((prev) => {
      const idx = CYCLE_ORDER.indexOf(prev);
      return CYCLE_ORDER[(idx + 1) % CYCLE_ORDER.length];
    });
  }, []);
  return { mode, cycle };
}

/** Consume prevalence display context. */
export function usePrevalenceDisplay(): PrevalenceDisplayContextValue {
  return useContext(PrevalenceDisplayContext);
}

/** Format a prevalence value according to the current display mode. */
export function formatPrevalence(
  count: number,
  total: number,
  mode: PrevalenceDisplayMode,
): string {
  if (total === 0) return "—";
  switch (mode) {
    case "fraction":
      return `${count}/${total}`;
    case "percent":
      return `${Math.round((count / total) * 100)}%`;
  }
}
