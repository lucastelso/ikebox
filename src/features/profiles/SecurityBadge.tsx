import { ShieldAlert, ShieldCheck, ShieldX } from 'lucide-react';
import type { Security, SecurityNote } from '@/bindings';
import { useT } from '@/i18n';
import { cn } from '@/lib/utils';

const STYLE: Record<Security, { icon: typeof ShieldCheck; color: string }> = {
  good: { icon: ShieldCheck, color: 'text-success' },
  legacy: { icon: ShieldAlert, color: 'text-warning' },
  weak: { icon: ShieldX, color: 'text-destructive' },
};

/**
 * Badge of a profile's security level. It only warns: the profile still
 * connects. `onHero` uses the colors of the connection screen's top.
 */
export function SecurityBadge({
  level,
  onHero = false,
  className,
}: {
  level: Security;
  onHero?: boolean;
  className?: string;
}) {
  const t = useT().profiles.security;
  const { icon: Icon, color } = STYLE[level];
  return (
    <span
      className={cn(
        'inline-flex w-fit shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-xs font-semibold',
        onHero ? 'bg-white/15 text-white' : cn('border border-current', color),
        className,
      )}
    >
      <Icon className="size-3.5" aria-hidden />
      {t.label[level]}
    </span>
  );
}

/** The profile's weaknesses, the worst first, each with its explanation. */
export function SecurityNotes({ notes }: { notes: SecurityNote[] }) {
  const t = useT().profiles.security;
  if (notes.length === 0) return null;
  return (
    <div className="grid gap-2 border-t px-4 py-3 text-xs">
      <p className="font-semibold">{t.title}</p>
      <p className="text-muted-foreground">{t.intro}</p>
      <ul className="grid gap-2">
        {notes.map((note) => {
          const { icon: Icon, color } = STYLE[note.level];
          return (
            <li key={note.message} className="flex gap-2">
              <Icon className={cn('mt-0.5 size-3.5 shrink-0', color)} aria-hidden />
              <span>
                <span className="sr-only">{t.label[note.level]}: </span>
                {note.message}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
