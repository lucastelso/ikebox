import {
  CircleCheck,
  CircleX,
  Languages,
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
import { type LanguagePref, useLanguage, useT } from '@/i18n';
import { applyTheme, readTheme, type Theme } from '@/lib/theme';
import { cn } from '@/lib/utils';
import { ComponentCard } from '../component/ComponentCard';
import { useComponent } from '../component/useComponent';

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

const THEMES: Array<{ value: Theme; icon: typeof Sun }> = [
  { value: 'system', icon: Monitor },
  { value: 'light', icon: Sun },
  { value: 'dark', icon: Moon },
];

/** Language names always in their own language. */
const LANGUAGES: Array<{ value: LanguagePref; name?: string }> = [
  { value: 'system' },
  { value: 'en', name: 'English' },
  { value: 'pt', name: 'Português' },
];

const OPTION =
  'flex flex-1 cursor-pointer flex-col items-center gap-1 rounded-lg border p-3 text-xs transition-colors hover:bg-muted has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring';

/** Network component, environment diagnostics, appearance, language and app info. */
export function SystemScreen() {
  const t = useT().system;
  const language = useLanguage();
  const component = useComponent();
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

  /** Instala/remove e, se mudou algo, refaz o diagnóstico. */
  async function changeComponent(action: () => Promise<boolean>) {
    if (await action()) void run();
  }

  return (
    <div className="flex w-full flex-col gap-4">
      <ComponentCard
        info={component.info}
        busy={component.busy}
        onInstall={() => void changeComponent(component.install)}
        onUninstall={() => void changeComponent(component.uninstall)}
      />

      <Card>
        <CardHeader className="flex-row items-start justify-between gap-2">
          <div className="grid gap-1.5">
            <CardTitle>{t.diagnostics}</CardTitle>
            <CardDescription>
              {checks === null ? t.checkingComputer : failures > 0 ? t.problems(failures) : t.ready}
            </CardDescription>
          </div>
          <Button size="sm" variant="outline" onClick={() => void run()} disabled={running}>
            {running ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
            {t.check}
          </Button>
        </CardHeader>
        <CardContent>
          <ul className="grid gap-3" aria-label={t.checks}>
            {checks?.map((c) => {
              const Icon = ICON[c.level];
              return (
                <li key={c.name} className="flex items-start gap-3 text-sm">
                  <Icon
                    className={cn('mt-0.5 size-4 shrink-0', TONE[c.level])}
                    aria-label={t.level[c.level]}
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
          <CardTitle>{t.appearance}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <fieldset className="flex gap-2">
            <legend className="sr-only">{t.theme}</legend>
            {THEMES.map(({ value, icon: Icon }) => (
              <label
                key={value}
                className={cn(
                  OPTION,
                  theme === value && 'border-primary bg-primary/10 text-primary',
                )}
              >
                <input
                  type="radio"
                  name="theme"
                  value={value}
                  checked={theme === value}
                  onChange={() => chooseTheme(value)}
                  className="sr-only"
                />
                <Icon className="size-4" aria-hidden />
                {value === 'system'
                  ? t.themeSystem
                  : value === 'light'
                    ? t.themeLight
                    : t.themeDark}
              </label>
            ))}
          </fieldset>
          <fieldset className="grid gap-2">
            <legend className="mb-2 flex items-center gap-1.5 text-sm font-medium">
              <Languages className="size-4 text-muted-foreground" aria-hidden />
              {t.language}
            </legend>
            <div className="flex gap-2">
              {LANGUAGES.map(({ value, name }) => (
                <label
                  key={value}
                  className={cn(
                    OPTION,
                    language.pref === value && 'border-primary bg-primary/10 text-primary',
                  )}
                >
                  <input
                    type="radio"
                    name="language"
                    value={value}
                    checked={language.pref === value}
                    onChange={() => language.setPref(value)}
                    className="sr-only"
                  />
                  {name ?? t.languageSystem}
                </label>
              ))}
            </div>
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
