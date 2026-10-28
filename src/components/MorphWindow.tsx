import { useCallback, useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { animate } from "animejs";
import { motion, useAnimeScope } from "../anim";

interface Props {
  /** Pulsante da cui la finestra nasce e in cui torna chiudendosi. */
  anchor: HTMLElement;
  label: string;
  onClose: () => void;
  /** Il contenuto riceve `close`, che fa rientrare la finestra nel pulsante. */
  children: (close: () => void) => ReactNode;
}

const GAP = 8;
const MARGIN = 12;

type Morph = (back: boolean, done?: () => void) => void;

/**
 * Finestrella che si apre trasformandosi dal pulsante che la chiama (che
 * intanto sparisce) e chiudendosi ci rientra. Esc e un clic fuori la chiudono.
 */
export function MorphWindow({ anchor, label, onClose, children }: Props) {
  // Posizione del pulsante all'apertura: la finestra ci torna anche se nel
  // frattempo il pulsante è stato sostituito (contatto appena salvato).
  const from = useRef(anchor.getBoundingClientRect());
  const place = useRef({ left: 0, top: 0, width: 1, height: 1 });
  const morph = useRef<Morph>((_back, done) => done?.());
  const closing = useRef(false);

  const box = useAnimeScope<HTMLDivElement>((scope) => {
    const el = scope.root as HTMLElement;
    const body = el.querySelector<HTMLElement>(".morph-window-body");
    const a = from.current;
    const p = place.current;
    // Trasformazione che porta la finestra esattamente sul pulsante.
    const sx = a.width / p.width;
    const sy = a.height / p.height;
    const start = { x: a.left - p.left, y: a.top - p.top, sx, sy, radius: `${8 / sx}px / ${8 / sy}px` };
    // Le durate qui sono esplicite, quindi il "meno movimento" va applicato a mano.
    const ms = (n: number) => (scope.matches.reducedMotion ? 0 : n);
    const run: Morph = (back, done) => {
      const fromTo = <T,>(open: T, closed: T): [T, T] => (back ? [open, closed] : [closed, open]);
      animate(el, {
        translateX: fromTo(0, start.x),
        translateY: fromTo(0, start.y),
        scaleX: fromTo(1, start.sx),
        scaleY: fromTo(1, start.sy),
        borderRadius: fromTo("10px / 10px", start.radius),
        duration: ms(back ? motion.duration.base : motion.duration.slow * 0.7),
        ease: back ? motion.ease.inOut : motion.ease.out,
        onComplete: () => done?.(),
      });
      if (body) {
        animate(body, {
          opacity: fromTo(1, 0),
          duration: ms(motion.duration.fast),
          delay: ms(back ? 0 : motion.duration.fast),
        });
      }
    };
    scope.add("morph", run);
    morph.current = (back, done) => scope.methods.morph(back, done);
    run(false);
  }, []);

  // Prima che la finestra si veda: posizione accanto al pulsante e forma del
  // pulsante, così il primo fotogramma parte da lì.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const a = from.current;
    const width = el.offsetWidth;
    const height = el.offsetHeight;
    const left = Math.max(MARGIN, Math.min(a.left, window.innerWidth - width - MARGIN));
    let top = a.bottom + GAP;
    if (top + height > window.innerHeight - MARGIN) top = Math.max(MARGIN, a.top - GAP - height);
    place.current = { left, top, width, height };
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
    el.style.transformOrigin = "0 0";
    el.style.transform = `translate(${a.left - left}px, ${a.top - top}px) scale(${a.width / width}, ${a.height / height})`;
    const body = el.querySelector<HTMLElement>(".morph-window-body");
    if (body) body.style.opacity = "0";
    anchor.style.visibility = "hidden";
    return () => {
      anchor.style.visibility = "";
    };
  }, [anchor, box]);

  const close = useCallback(() => {
    if (closing.current) return;
    closing.current = true;
    morph.current(true, () => {
      onClose();
      if (anchor.isConnected) anchor.focus({ preventScroll: true });
    });
  }, [anchor, onClose]);

  // Esc o un clic fuori chiudono, come i menu.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      close();
    };
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("mousedown", onDown);
    };
  }, [box, close]);

  return createPortal(
    <div ref={box} className="morph-window" role="dialog" aria-label={label}>
      <div className="morph-window-body">{children(close)}</div>
    </div>,
    document.body,
  );
}
