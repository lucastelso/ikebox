import { List, Power } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { Toaster, toast } from 'sonner';
import { commands, events, type ImportPreview } from './bindings';
import { HomeScreen } from './features/connection/HomeScreen';
import { ImportDialog } from './features/profiles/ImportDialog';
import { ProfilesScreen } from './features/profiles/ProfilesScreen';
import { cn } from './lib/utils';

type View = 'connection' | 'profiles';

const NAV: Array<{ view: View; label: string; icon: typeof Power }> = [
  { view: 'connection', label: 'Conexão', icon: Power },
  { view: 'profiles', label: 'Perfis', icon: List },
];

/** Janela principal: navegação, telas e importação (botão ou arrastar arquivo). */
export function App() {
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
    <div className="flex min-h-full flex-col">
      <header className="flex items-center gap-3 border-b px-5 py-3">
        <img src="/logo.svg" alt="" className="size-7" />
        <h1 className="flex-1 text-base font-semibold tracking-tight">IPsec Client</h1>
        <nav className="flex rounded-lg bg-muted p-1" aria-label="Seções">
          {NAV.map(({ view: v, label, icon: Icon }) => (
            <button
              key={v}
              type="button"
              onClick={() => setView(v)}
              aria-current={view === v ? 'page' : undefined}
              className={cn(
                'flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-muted-foreground transition-colors',
                view === v && 'bg-card text-foreground shadow-sm',
              )}
            >
              <Icon className="size-3.5" aria-hidden />
              {label}
            </button>
          ))}
        </nav>
      </header>
      <main className="mx-auto flex w-full max-w-md flex-1 flex-col items-center gap-6 p-5">
        {view === 'connection' ? (
          <HomeScreen key={revision} onImport={() => setView('profiles')} />
        ) : (
          <ProfilesScreen key={revision} onImport={() => void chooseFile()} />
        )}
      </main>
      {preview && (
        <ImportDialog
          preview={preview}
          onClose={() => setPreview(null)}
          onImported={(profile) => {
            setPreview(null);
            setRevision((r) => r + 1);
            toast.success(`Perfil "${profile.name}" importado`, {
              description: 'Na próxima conexão, informe suas credenciais.',
            });
          }}
        />
      )}
      <Toaster position="bottom-center" richColors closeButton />
    </div>
  );
}
