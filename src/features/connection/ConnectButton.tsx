import { LoaderCircle, Power } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { Phase } from './connectionReducer';

const LABEL: Record<Phase, string> = {
  idle: 'Conectar',
  preparing: 'Cancelar',
  negotiating: 'Cancelar',
  connected: 'Desconectar',
  reconnecting: 'Desconectar',
  disconnecting: 'Desconectando…',
};

const RING: Record<Phase, string> = {
  idle: 'ring-border bg-card text-muted-foreground hover:text-primary hover:ring-primary/50',
  preparing: 'ring-warning/60 bg-card text-warning animate-pulse',
  negotiating: 'ring-warning/60 bg-card text-warning animate-pulse',
  connected: 'ring-success bg-success/10 text-success hover:bg-success/15',
  reconnecting: 'ring-warning bg-warning/10 text-warning animate-pulse',
  disconnecting: 'ring-border bg-card text-muted-foreground',
};

interface Props {
  phase: Phase;
  disabled?: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
}

/** Botão central: conecta, cancela ou desconecta conforme a fase. */
export function ConnectButton({ phase, disabled, onConnect, onDisconnect }: Props) {
  const busy = phase === 'preparing' || phase === 'negotiating' || phase === 'disconnecting';
  const action = phase === 'idle' ? onConnect : onDisconnect;
  return (
    <button
      type="button"
      onClick={action}
      disabled={disabled || phase === 'disconnecting'}
      aria-label={LABEL[phase]}
      className={cn(
        'group flex size-40 flex-col items-center justify-center gap-2 rounded-full ring-8 transition-all duration-300',
        'focus-visible:outline-none focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60',
        RING[phase],
      )}
    >
      {busy ? (
        <LoaderCircle className="size-12 animate-spin" aria-hidden />
      ) : (
        <Power className="size-12" strokeWidth={2.25} aria-hidden />
      )}
      <span className="text-sm font-semibold">{LABEL[phase]}</span>
    </button>
  );
}
