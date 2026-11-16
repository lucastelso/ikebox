import {
  CircleCheck,
  CircleX,
  LoaderCircle,
  Monitor,
  Moon,
  RefreshCw,
  Sun,
  TriangleAlert,
} from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { type AppInfo, type CheckLevel, commands, type DiagnosticCheck } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { applyTheme, readTheme, type Theme } from '@/lib/theme';
import { cn } from '@/lib/utils';

const ICON: Record<CheckLevel, typeof CircleCheck> = {
  ok: CircleCheck,
  warn: TriangleAlert,
  fail: CircleX,
};

const TONE: Record<CheckLevel, string> = {
  ok: 'text-success',
  warn: 'text-warning',
  fail: 'text-destructive',
};

const THEMES: Array<{ value: Theme; label: string; icon: typeof Sun }> = [
  { value: 'system', label: 'Sistema', icon: Monitor },
  { value: 'light', label: 'Claro', icon: Sun },
  { value: 'dark', label: 'Escuro', icon: Moon },
];

/** Diagnóstico do ambiente, aparência e informações do app. */
export function SystemScreen() {
  const [checks, setChecks] = useState<DiagnosticCheck[] | null>(null);
  const [running, setRunning] = useState(false);
  const [theme, setTheme] = useState<Theme>(readTheme);
  const [info, setInfo] = useState<AppInfo | null>(null);

  const run = useCallback(async () => {
    setRunning(true);
    try {
      setChecks(await commands.runDiagnostics());
    } finally {
      setRunning(false);
    }
  }, []);

  useEffect(() => {
    void run();
    commands
      .appInfo()
      .then(setInfo)
      .catch(() => {});
  }, [run]);

  function chooseTheme(value: Theme) {
    setTheme(value);
    applyTheme(value);
  }

  const failures = checks?.filter((c) => c.level === 'fail').length ?? 0;

  return (
    <div className="flex w-full flex-col gap-4">
      <Card>
        <CardHeader className="flex-row items-start justify-between gap-2">
          <div className="grid gap-1.5">
            <CardTitle>Diagnóstico</CardTitle>
            <CardDescription>
              {checks === null
                ? 'Verificando o computador…'
                : failures > 0
                  ? `${failures} problema(s) impedem a conexão.`
                  : 'Pronto para conectar.'}
            </CardDescription>
          </div>
          <Button size="sm" variant="outline" onClick={() => void run()} disabled={running}>
            {running ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
            Verificar
          </Button>
        </CardHeader>
        <CardContent>
          <ul className="grid gap-3" aria-label="Verificações">
            {checks?.map((c) => {
              const Icon = ICON[c.level];
              return (
                <li key={c.name} className="flex items-start gap-3 text-sm">
                  <Icon
                    className={cn('mt-0.5 size-4 shrink-0', TONE[c.level])}
                    aria-label={c.level}
                  />
                  <div className="min-w-0" data-selectable>
                    <p className="font-medium">{c.name}</p>
                    <p className="break-words text-xs text-muted-foreground">{c.detail}</p>
                    {c.hint && c.level !== 'ok' && (
                      <p className="mt-1 break-words text-xs">→ {c.hint}</p>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Aparência</CardTitle>
        </CardHeader>
        <CardContent>
          <fieldset className="flex gap-2">
            <legend className="sr-only">Tema</legend>
            {THEMES.map(({ value, label, icon: Icon }) => (
              <label
                key={value}
                className={cn(
                  'flex flex-1 cursor-pointer flex-col items-center gap-1 rounded-lg border p-3 text-xs transition-colors hover:bg-muted has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring',
                  theme === value && 'border-primary bg-primary/10 text-primary',
                )}
              >
                <input
                  type="radio"
                  name="tema"
                  value={value}
                  checked={theme === value}
                  onChange={() => chooseTheme(value)}
                  className="sr-only"
                />
                <Icon className="size-4" aria-hidden />
                {label}
              </label>
            ))}
          </fieldset>
        </CardContent>
      </Card>

      {info && (
        <p className="text-center text-xs text-muted-foreground" data-selectable>
          IPsec Client {info.version} · engine {info.engineImage}
        </p>
      )}
    </div>
  );
}
