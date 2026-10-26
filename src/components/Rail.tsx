import { Mail, Plus } from "lucide-react";
import type { Account } from "../api";
import { avatarHue, initials } from "../format";

interface Props {
  accounts: Account[];
  current: string | null;
  /** Da leggere per casella: posta in arrivo più chat. */
  badges: Record<string, number>;
  onSelect: (accountId: string) => void;
  onAdd: () => void;
}

/** Colonna degli spazi di lavoro: una casella per icona. */
export function Rail({ accounts, current, badges, onSelect, onAdd }: Props) {
  return (
    <nav className="rail" aria-label="Caselle">
      <div className="rail-brand" title="Postamon">
        <span className="brand-mark">
          <Mail />
        </span>
      </div>
      <div className="rail-list">
        {accounts.map((a) => {
          const count = badges[a.id] ?? 0;
          return (
            <button
              key={a.id}
              className={`rail-item ${a.id === current ? "active" : ""}`}
              title={a.name ? `${a.name}\n${a.email}` : a.email}
              aria-label={`Casella ${a.email}${count ? `, ${count} da leggere` : ""}`}
              onClick={() => onSelect(a.id)}
            >
              <span className="rail-avatar" style={{ background: `hsl(${avatarHue(a.email)} 42% 40%)` }}>
                {initials(a.name, a.email)}
              </span>
              {count > 0 && <span className="rail-badge">{count > 99 ? "99+" : count}</span>}
            </button>
          );
        })}
      </div>
      <button className="rail-item rail-add" title="Aggiungi casella" aria-label="Aggiungi casella" onClick={onAdd}>
        <Plus />
      </button>
    </nav>
  );
}
