// Animazioni con anime.js v4: lo stesso wrapper dei progetti ELEFIX, così le
// animazioni si somigliano e rispettano tutte `prefers-reduced-motion`.
//
// - partono solo dopo il mount, dentro `useEffect`;
// - se Windows chiede meno movimento le durate vanno a zero: gli elementi
//   saltano allo stato finale invece di restare a metà (o invisibili);
// - allo smontaggio lo scope viene revertito, niente animazioni orfane.
//
// Dentro `setup` i selettori sono cercati solo dentro la radice e l'API di
// anime.js (`animate`, `createTimeline`, ...) resta usabile così com'è.

import { createScope, type Scope } from "animejs";
import { useEffect, useRef, type DependencyList, type RefObject } from "react";

export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/** Durate (ms), easing e ritardi condivisi: prima di inventarne uno, guarda qui. */
export const motion = {
  duration: {
    /** Micro-feedback: hover, toggle, focus. */
    fast: 160,
    /** Default: entrate, pannelli, cambi di stato. */
    base: 280,
    /** Movimenti ampi: finestre che si aprono. */
    slow: 520,
  },
  ease: {
    out: "outQuart",
    inOut: "inOutQuad",
    in: "inQuad",
    back: "outBack(1.4)",
  },
  stagger: {
    tight: 30,
    base: 60,
  },
} as const;

export function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(REDUCED_MOTION_QUERY).matches;
}

/** Riceve lo scope (per `scope.matches`, `scope.add("nome", fn)`) e può restituire un cleanup. */
export type AnimeSetup = (scope: Scope) => void | (() => void);

/** Scope anime.js legato al ciclo di vita del componente; la ref va sul nodo radice. */
export function useAnimeScope<T extends HTMLElement = HTMLDivElement>(
  setup: AnimeSetup,
  deps: DependencyList = [],
): RefObject<T | null> {
  const root = useRef<T>(null);

  useEffect(() => {
    if (!root.current) return;

    const scope = createScope({
      root,
      defaults: { duration: motion.duration.base, ease: motion.ease.out },
      mediaQueries: { reducedMotion: REDUCED_MOTION_QUERY },
    }).add((self) => {
      const s = self as Scope;
      // `s.defaults` va mutato, non riassegnato: è l'oggetto che anime.js usa
      // come default per le animazioni create qui dentro.
      if (s.matches.reducedMotion) {
        Object.assign(s.defaults, { duration: 0, delay: 0 });
      }
      return setup(s);
    });

    return () => scope.revert();
    // Il ri-arming dipende da `deps`, non dalla closure di `setup`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return root;
}
