import { LoaderCircle, Power } from 'lucide-react';
import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

/**
 * The main action, inside the colored top of the connection screen: solid
 * (white) to start something, translucent to stop what is running.
 */
export function HeroAction({
  label,
  onClick,
  solid,
  busy = false,
  disabled = false,
  icon,
}: {
  label: string;
  onClick: () => void;
  solid: boolean;
  busy?: boolean;
  disabled?: boolean;
  icon?: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'flex h-12 w-full items-center justify-center gap-2 rounded-2xl text-base font-bold transition-colors',
        'outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white',
        'disabled:cursor-not-allowed disabled:opacity-60 [&_svg]:size-5',
        solid
          ? 'bg-white text-[var(--hero-2)] hover:bg-white/90'
          : 'bg-white/15 text-white hover:bg-white/25',
      )}
    >
      {busy ? (
        <LoaderCircle className="animate-spin" aria-hidden />
      ) : (
        (icon ?? <Power aria-hidden />)
      )}
      {label}
    </button>
  );
}
