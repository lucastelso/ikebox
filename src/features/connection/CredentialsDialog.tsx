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

const FIELDS: Record<
  CredentialField,
  { label: string; secret: boolean; key: keyof CredentialsInput; placeholder: string }
> = {
  psk: {
    label: 'PSK do grupo',
    secret: true,
    key: 'psk',
    placeholder: 'chave compartilhada do grupo',
  },
  xauthUser: {
    label: 'Usuário',
    secret: false,
    key: 'xauthUser',
    placeholder: 'ex.: sua matrícula',
  },
  xauthPassword: {
    label: 'Senha',
    secret: true,
    key: 'xauthPassword',
    placeholder: 'sua senha da VPN',
  },
};

const ORDER: CredentialField[] = ['xauthUser', 'xauthPassword', 'psk'];

function SecretInput({ id, ...props }: React.ComponentProps<typeof Input>) {
  const [visible, setVisible] = useState(false);
  return (
    <div className="relative">
      <Input id={id} type={visible ? 'text' : 'password'} className="pr-10" {...props} />
      <button
        type="button"
        onClick={() => setVisible((v) => !v)}
        aria-label={visible ? 'Ocultar' : 'Mostrar'}
        className="absolute inset-y-0 right-0 flex w-10 items-center justify-center text-muted-foreground hover:text-foreground"
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
  keyringAvailable,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profileName: string;
  fields: CredentialField[];
  keyringAvailable: boolean;
  onSubmit: (input: CredentialsInput, remember: boolean) => void;
}) {
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
              Credenciais
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
              <Label htmlFor={`${baseId}-remember`}>Lembrar no chaveiro do sistema</Label>
              {!keyringAvailable && (
                <p className="text-xs text-muted-foreground">
                  Chaveiro indisponível: as credenciais serão pedidas a cada conexão.
                </p>
              )}
            </div>
          </div>

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancelar
            </Button>
            <Button type="submit" disabled={!complete}>
              Conectar
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
