import { List, Power, Settings } from 'lucide-react';
import type * as React from 'react';
import { useCallback, useEffect, useState } from 'react';
import { Toaster, toast } from 'sonner';
import { commands, events, type ImportPreview } from './bindings';
import { HomeScreen } from './features/connection/HomeScreen';
import { ImportDialog } from './features/profiles/ImportDialog';
import { ProfilesScreen } from './features/profiles/ProfilesScreen';
import { SystemScreen } from './features/system/SystemScreen';
import { useT } from './i18n';
import { cn } from './lib/utils';

type View = 'connection' | 'profiles' | 'system';

const NAV: Array<{ view: View; icon: typeof Power }> = [
  { view: 'connection', icon: Power },
  { view: 'profiles', icon: List },
  { view: 'system', icon: Settings },
];

/** Content of the Profiles and System tabs, with an optional page title. */
function Page({ title, children }: { title?: string; children: React.ReactNode }) {
  return (
    <div className="mx-auto flex w-full max-w-md flex-col gap-4 p-5">
      {title && <h1 className="text-xl font-bold tracking-tight">{title}</h1>}
      {children}
    </div>
  );
}

/** Janela principal: navegação, telas e importação (botão ou arrastar arquivo). */
export function App() {
  const t = useT();
  const [view, setView] = useState<View>('connection');
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  // Muda a cada importação para as telas recarregarem a lista.
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    events.importPreview
      .listen((event) => {
        setView('profiles');
        setPreview(event.payload);
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const chooseFile = useCallback(async () => {
    const result = await commands.chooseImportFile();
    if (result.status === 'error') toast.error(result.error.message);
    else if (result.data) setPreview(result.data);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <main className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {view === 'connection' && (
          <HomeScreen key={revision} onImport={() => setView('profiles')} />
        )}
        {view === 'profiles' && (
          <Page>
            <ProfilesScreen key={revision} onImport={() => void chooseFile()} />
          </Page>
        )}
        {view === 'system' && (
          <Page title={t.nav.system}>
            <SystemScreen />
          </Page>
        )}
      </main>
      <nav
        className="grid grid-cols-3 gap-1 border-t bg-card px-3 pb-2 pt-1.5"
        aria-label={t.nav.label}
      >
        {NAV.map(({ view: v, icon: Icon }) => (
          <button
            key={v}
            type="button"
            onClick={() => setView(v)}
            aria-current={view === v ? 'page' : undefined}
            className={cn(
              'flex flex-col items-center gap-0.5 rounded-xl py-1.5 text-xs font-semibold text-muted-foreground transition-colors hover:text-foreground',
              'outline-none focus-visible:ring-2 focus-visible:ring-ring',
              view === v && 'bg-primary/12 text-primary hover:text-primary',
            )}
          >
            <Icon className="size-5" aria-hidden />
            {t.nav[v]}
          </button>
        ))}
      </nav>
      {preview && (
        <ImportDialog
          preview={preview}
          onClose={() => setPreview(null)}
          onImported={(profile) => {
            setPreview(null);
            setRevision((r) => r + 1);
            toast.success(t.app.imported(profile.name), {
              description: t.app.importedHint,
            });
          }}
        />
      )}
      <Toaster position="top-center" richColors closeButton />
    </div>
  );
}
