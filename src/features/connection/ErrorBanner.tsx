import { CircleAlert, LoaderCircle, X } from 'lucide-react';
import type { UiError } from '@/bindings';
import { Button } from '@/components/ui/button';

/** Erros que se resolvem informando as credenciais de novo. */
export function isCredentialError(error: UiError): boolean {
  if (error.kind === 'missingCredentials') return true;
  if (error.kind !== 'negotiation') return false;
  return /XAuth|PSK|fase 1/i.test(error.message);
}

/** Erro com causa provável, detalhes recolhíveis e ação de correção. */
export function ErrorBanner({
  error,
  onDismiss,
  onFixCredentials,
  onInstallComponent,
  installing = false,
}: {
  error: UiError;
  onDismiss: () => void;
  onFixCredentials?: () => void;
  /** Oferecido quando falta o componente de rede. */
  onInstallComponent?: () => void;
  installing?: boolean;
}) {
  return (
    <div
      role="alert"
      className="w-full rounded-xl border border-destructive/40 bg-destructive/10 p-4 text-sm"
    >
      <div className="flex items-start gap-3">
        <CircleAlert className="mt-0.5 size-5 shrink-0 text-destructive" aria-hidden />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <p className="font-medium">{error.message}</p>
          {error.details.length > 0 && (
            <details className="text-muted-foreground">
              <summary className="cursor-pointer select-none text-xs">Detalhes</summary>
              <ul className="mt-2 flex flex-col gap-1 text-xs" data-selectable>
                {error.details.map((d) => (
                  <li key={d} className="break-words font-mono">
                    {d}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {onInstallComponent && error.kind === 'componentMissing' && (
            <div>
              <Button size="sm" onClick={onInstallComponent} disabled={installing}>
                {installing && <LoaderCircle className="animate-spin" />}
                {installing ? 'Instalando…' : 'Instalar componente de rede'}
              </Button>
            </div>
          )}
          {onFixCredentials && isCredentialError(error) && (
            <div>
              <Button size="sm" variant="outline" onClick={onFixCredentials}>
                Informar credenciais
              </Button>
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Fechar aviso"
          className="rounded-sm opacity-60 hover:opacity-100"
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  );
}
