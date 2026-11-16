import { LoaderCircle, ShieldCheck } from 'lucide-react';
import type { ComponentInfo } from '@/bindings';
import { Button } from '@/components/ui/button';
import { installLabel } from './useComponent';

const TEXT = {
  notInstalled: {
    title: 'Falta um passo: instalar o componente de rede',
    body: 'É feito uma vez, com a senha de administrador. Depois, conectar não pede senha.',
  },
  outdated: {
    title: 'Atualização do componente de rede',
    body: 'Este app traz uma versão diferente do componente de rede.',
  },
  broken: {
    title: 'O componente de rede não está respondendo',
    body: 'Reinstale para voltar a conectar.',
  },
} as const;

/** Aviso na tela principal quando o componente de rede precisa de atenção. */
export function SetupBanner({
  info,
  busy,
  onInstall,
}: {
  info: ComponentInfo;
  busy: boolean;
  onInstall: () => void;
}) {
  if (!info.canInstall || info.state === 'ready') return null;
  const text = TEXT[info.state];
  return (
    <section
      aria-label="Componente de rede"
      className="w-full rounded-xl border border-primary/40 bg-primary/10 p-4 text-sm"
    >
      <div className="flex items-start gap-3">
        <ShieldCheck className="mt-0.5 size-5 shrink-0 text-primary" aria-hidden />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <p className="font-medium">{text.title}</p>
          <p className="text-xs text-muted-foreground">{text.body}</p>
          <div>
            <Button size="sm" onClick={onInstall} disabled={busy}>
              {busy && <LoaderCircle className="animate-spin" />}
              {busy ? 'Instalando…' : `${installLabel(info)} agora`}
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
