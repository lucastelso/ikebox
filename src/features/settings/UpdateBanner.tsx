import { ArrowUpCircle } from 'lucide-react';
import { useState } from 'react';
import { commands } from '@/bindings';
import { Button } from '@/components/ui/button';
import { useT } from '@/i18n';
import { useAppSettings } from './useAppSettings';

/** A newer version was found by the optional update check. */
export function UpdateBanner() {
  const t = useT().update;
  const { settings } = useAppSettings();
  const [error, setError] = useState<string | null>(null);
  const version = settings?.availableUpdate;
  if (!version) return null;
  async function open() {
    const result = await commands.openReleasePage();
    setError(result.status === 'error' ? result.error.message : null);
  }
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 p-3 text-sm">
      <ArrowUpCircle className="size-4 shrink-0 text-primary" aria-hidden />
      <span className="flex-1">{t.available(version)}</span>
      <Button type="button" size="sm" variant="secondary" onClick={() => void open()}>
        {t.view}
      </Button>
      {error && (
        <p role="alert" className="w-full text-xs text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
