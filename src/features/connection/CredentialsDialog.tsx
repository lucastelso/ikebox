import { Eye, EyeOff, KeyRound } from 'lucide-react';
import { type FormEvent, useEffect, useId, useState } from 'react';
import type { CredentialField, CredentialsInput } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useT } from '@/i18n';
import type { Messages } from '@/i18n/messages';

function fieldSpecs(
  t: Messages['credentials'],
  groupPsk: boolean,
): Record<
  CredentialField,
  { label: string; secret: boolean; key: keyof CredentialsInput; placeholder: string }
> {
  return {
    // With XAuth the PSK is the group's; without it, just "the PSK".
    psk: groupPsk
      ? { label: t.psk, secret: true, key: 'psk', placeholder: t.pskPlaceholder }
      : { label: t.pskOnly, secret: true, key: 'psk', placeholder: t.pskOnlyPlaceholder },
    xauthUser: { label: t.user, secret: false, key: 'xauthUser', placeholder: t.userPlaceholder },
    xauthPassword: {
      label: t.password,
      secret: true,
      key: 'xauthPassword',
      placeholder: t.passwordPlaceholder,
    },
  };
}

const ORDER: CredentialField[] = ['xauthUser', 'xauthPassword', 'psk'];

function SecretInput({ id, ...props }: React.ComponentProps<typeof Input>) {
  const t = useT().credentials;
  const [visible, setVisible] = useState(false);
  return (
    <div className="relative">
      <Input id={id} type={visible ? 'text' : 'password'} className="pr-10" {...props} />
      <button
        type="button"
        onClick={() => setVisible((v) => !v)}
        aria-label={visible ? t.hide : t.show}
        className="absolute inset-y-0 right-0 flex w-10 items-center justify-center rounded-r-lg text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        {visible ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
      </button>
    </div>
  );
}

/**
 * Pede as credenciais que faltam. Os valores vão direto para o Rust no
 * `connect` e não ficam guardados na interface depois do envio.
 */
export function CredentialsDialog({
  open,
  onOpenChange,
  profileName,
  fields,
  groupPsk = true,
  keyringAvailable,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profileName: string;
  fields: CredentialField[];
  /** The profile uses XAuth, so the PSK is the group's. */
  groupPsk?: boolean;
  keyringAvailable: boolean;
  onSubmit: (input: CredentialsInput, remember: boolean) => void;
}) {
  const t = useT().credentials;
  const FIELDS = fieldSpecs(t, groupPsk);
  const baseId = useId();
  const [values, setValues] = useState<CredentialsInput>({});
  const [remember, setRemember] = useState(keyringAvailable);

  // Ao fechar, apaga o que foi digitado.
  useEffect(() => {
    if (!open) setValues({});
  }, [open]);
  useEffect(() => setRemember(keyringAvailable), [keyringAvailable]);

  const ordered = ORDER.filter((f) => fields.includes(f));
  const complete = ordered.every((f) => (values[FIELDS[f].key] ?? '').length > 0);

  function submit(e: FormEvent) {
    e.preventDefault();
    if (!complete) return;
    onSubmit(values, remember && keyringAvailable);
    setValues({});
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <KeyRound className="size-5 text-primary" aria-hidden />
              {t.title}
            </DialogTitle>
            <DialogDescription>{profileName}</DialogDescription>
          </DialogHeader>

          <div className="grid gap-4">
            {ordered.map((field, index) => {
              const spec = FIELDS[field];
              const id = `${baseId}-${field}`;
              const common = {
                id,
                value: values[spec.key] ?? '',
                placeholder: spec.placeholder,
                autoFocus: index === 0,
                autoComplete: 'off',
                spellCheck: false,
                onChange: (e: React.ChangeEvent<HTMLInputElement>) =>
                  setValues((v) => ({ ...v, [spec.key]: e.target.value })),
              };
              return (
                <div key={field} className="grid gap-2">
                  <Label htmlFor={id}>{spec.label}</Label>
                  {spec.secret ? <SecretInput {...common} /> : <Input {...common} />}
                </div>
              );
            })}
          </div>

          <div className="flex items-start gap-2">
            <Checkbox
              id={`${baseId}-remember`}
              checked={remember && keyringAvailable}
              disabled={!keyringAvailable}
              onCheckedChange={(c) => setRemember(c === true)}
            />
            <div className="grid gap-1">
              <Label htmlFor={`${baseId}-remember`}>{t.remember}</Label>
              {!keyringAvailable && <p className="text-xs text-muted-foreground">{t.noKeyring}</p>}
            </div>
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t.cancel}
            </Button>
            <Button type="submit" disabled={!complete}>
              {t.connect}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
