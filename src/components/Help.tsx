import { useEffect, useRef, useState, type ReactNode } from "react";
import { CircleQuestionMark } from "lucide-react";

/**
 * Spiegazione dietro un'icona "?": compare dopo 1,5 s di passaggio del mouse
 * o subito al clic (e resta finché non si clicca altrove).
 */
export function Help({ children, align = "right" }: { children: ReactNode; align?: "right" | "left" }) {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const root = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!pinned) return;
    const close = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) {
        setPinned(false);
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [pinned]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <span
      className="help"
      ref={root}
      onMouseEnter={() => {
        timer.current = window.setTimeout(() => setOpen(true), 1500);
      }}
      onMouseLeave={() => {
        window.clearTimeout(timer.current);
        if (!pinned) setOpen(false);
      }}
    >
      <button
        type="button"
        className="help-btn"
        aria-label="Spiegazione"
        aria-expanded={open}
        onClick={() => {
          window.clearTimeout(timer.current);
          setPinned(!pinned);
          setOpen(!pinned);
        }}
      >
        <CircleQuestionMark />
      </button>
      {open && (
        <span className={`help-pop ${align === "left" ? "left" : ""}`} role="tooltip">
          {children}
        </span>
      )}
    </span>
  );
}
