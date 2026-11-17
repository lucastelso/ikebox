import { ChevronRight, CircleAlert, LoaderCircle, X } from 'lucide-react';
import type { UiError } from '@/bindings';
import { Button } from '@/components/ui/button';
import { useT } from '@/i18n';

/** Erros que se resolvem informando as credenciais de novo. */
export function isCredentialError(error: UiError): boolean {
  if (error.kind === 'missingCredentials') return true;
  if (error.kind !== 'negotiation') return false;
  return /XAuth|PSK|fase 1|phase 1/i.test(error.message);
}

/** Erros que se resolvem instalando ou atualizando o componente de rede. */
export function isComponentError(error: UiError): boolean {
  return error.kind === 'componentMissing' || error.kind === 'componentOutdated';
}

/**
 * Splits a message from Rust into what happened and what to do: the probable
 * causes come as "what happened: what to check". Without a colon, it is all
 * "what happened".
 */
export function splitMessage(message: string): { what: string; next: string | null } {
  const at = message.indexOf(': ');
  if (at <= 0) return { what: message, next: null };
  const next = message.slice(at + 2).trim();
  return { what: message.slice(0, at), next: next.charAt(0).toUpperCase() + next.slice(1) };
}

/**
 * What went wrong, what to do next and the technical details (folded). The
 * action that fixes it lives in the colored top of the screen; this card only
 * repeats it when asked (`onFixCredentials`, `onInstallComponent`).
 */
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
  /** Offered when the network component is missing or old. */
  onInstallComponent?: () => void;
  installing?: boolean;
}) {
  const t = useT().error;
  const { what, next } = splitMessage(error.message);
  return (
    <div
      role="alert"
      className="w-full rounded-2xl border border-destructive/40 bg-card p-4 text-sm"
    >
      <div className="flex items-start gap-3">
        <CircleAlert className="mt-0.5 size-5 shrink-0 text-destructive" aria-hidden />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <p className="font-semibold">{what}</p>
          {next && <p className="text-muted-foreground">{next}</p>}
          {error.details.length > 0 && (
            <details className="group mt-1 text-muted-foreground">
              <summary className="flex w-fit cursor-pointer select-none items-center gap-1 rounded text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring">
                <ChevronRight
                  className="size-3.5 transition-transform group-open:rotate-90"
                  aria-hidden
                />
                {t.details}
              </summary>
              <ul className="mt-2 flex flex-col gap-1 text-xs" data-selectable>
                {error.details.map((d) => (
                  <li key={d} className="break-words font-mono">
                    {d}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {onInstallComponent && isComponentError(error) && (
            <div className="mt-1">
              <Button size="sm" onClick={onInstallComponent} disabled={installing}>
                {installing && <LoaderCircle className="animate-spin" />}
                {installing
                  ? t.installing
                  : error.kind === 'componentOutdated'
                    ? t.updateComponent
                    : t.installComponent}
              </Button>
            </div>
          )}
          {onFixCredentials && isCredentialError(error) && (
            <div className="mt-1">
              <Button size="sm" variant="outline" onClick={onFixCredentials}>
                {t.enterCredentials}
              </Button>
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={onDismiss}
          aria-label={t.dismiss}
          className="rounded-md p-0.5 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  );
}
