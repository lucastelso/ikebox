import { ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { type AppInfo, commands } from './bindings';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './components/ui/card';

/** Tela inicial provisória do scaffold: prova o IPC tipado com o Rust. */
export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands
      .appInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(String(e)));
  }, []);

  return (
    <main className="flex min-h-full flex-col items-center justify-center gap-6 p-6">
      <div className="flex items-center gap-3">
        <ShieldCheck className="size-10 text-primary" aria-hidden />
        <h1 className="text-2xl font-semibold tracking-tight">IPsec Client</h1>
      </div>
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Em construção</CardTitle>
          <CardDescription>A interface gráfica está sendo montada.</CardDescription>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground" data-selectable>
          {error && <p className="text-destructive">Erro: {error}</p>}
          {info && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              <dt>Versão</dt>
              <dd className="text-foreground">{info.version}</dd>
              <dt>Engine</dt>
              <dd className="truncate text-foreground">{info.engineImage}</dd>
            </dl>
          )}
        </CardContent>
      </Card>
    </main>
  );
}
