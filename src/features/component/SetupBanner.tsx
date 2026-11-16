import { LoaderCircle, ShieldCheck } from 'lucide-react';
import type { ComponentInfo } from '@/bindings';
import { Button } from '@/components/ui/button';
import { useT } from '@/i18n';
import { installLabel } from './useComponent';

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
  const t = useT().component;
  if (!info.canInstall || info.state === 'ready') return null;
  const text = t.banner[info.state];
  return (
    <section
      aria-label={t.title}
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
              {busy ? t.installing : t.now(installLabel(info, t))}
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
