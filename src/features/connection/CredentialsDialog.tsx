import { Eye, EyeOff, FileKey, KeyRound, Search, Usb } from 'lucide-react';
import { type FormEvent, useCallback, useEffect, useId, useState } from 'react';
import {
  type CredentialField,
  type CredentialsInput,
  commands,
  type ModuleChoice,
  type P12Choice,
  type TokenCertList,
} from '@/bindings';
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

/**
 * Fields typed by the person. The certificate is imported from a file; the
 * token's certificate and PIN (ADR-0029) have their own section.
 */
type TypedField = Exclude<CredentialField, 'clientCertificate' | 'tokenCertificate' | 'tokenPin'>;

function fieldSpecs(
  t: Messages['credentials'],
  groupPsk: boolean,
  otp: boolean,
): Record<
  TypedField,
  { label: string; secret: boolean; key: keyof CredentialsInput; placeholder: string }
> {
  return {
    // With XAuth the PSK is the group's; without it, just "the PSK".
    psk: groupPsk
      ? { label: t.psk, secret: true, key: 'psk', placeholder: t.pskPlaceholder }
      : { label: t.pskOnly, secret: true, key: 'psk', placeholder: t.pskOnlyPlaceholder },
    xauthUser: { label: t.user, secret: false, key: 'xauthUser', placeholder: t.userPlaceholder },
    // EAP asks for the same things as XAuth: the person's username and password.
    eapIdentity: {
      label: t.user,
      secret: false,
      key: 'eapIdentity',
      placeholder: t.userPlaceholder,
    },
    eapPassword: {
      label: t.password,
      secret: true,
      key: 'eapPassword',
      placeholder: t.passwordPlaceholder,
    },
    xauthPassword: {
      label: otp ? t.otpCode : t.password,
      secret: true,
      key: 'xauthPassword',
      placeholder: t.passwordPlaceholder,
    },
  };
}

const ORDER: TypedField[] = ['eapIdentity', 'eapPassword', 'xauthUser', 'xauthPassword', 'psk'];

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
 * Asks for the missing credentials. The values go straight to Rust in
 * `connect` and are not kept in the interface after sending.
 */
export function CredentialsDialog({
  open,
  onOpenChange,
  profileId,
  profileName,
  fields,
  groupPsk = true,
  otp = false,
  keyringAvailable,
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profileId: string;
  profileName: string;
  fields: CredentialField[];
  /** The profile uses XAuth, so the PSK is the group's. */
  groupPsk?: boolean;
  /** The XAuth password is a one-time code: never stored. */
  otp?: boolean;
  keyringAvailable: boolean;
  onSubmit: (input: CredentialsInput, remember: boolean) => void;
}) {
  const t = useT().credentials;
  const FIELDS = fieldSpecs(t, groupPsk, otp);
  const baseId = useId();
  const [values, setValues] = useState<CredentialsInput>({});
  const [remember, setRemember] = useState(keyringAvailable);
  // The client certificate: a .p12 picked in Rust's dialog (a token, not a
  // path) and its password, imported into the keyring before connecting.
  const needsCert = fields.includes('clientCertificate');
  const [p12, setP12] = useState<P12Choice | null>(null);
  const [p12Password, setP12Password] = useState('');
  const [certError, setCertError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The certificate on a smartcard or token: chosen from what the tokens
  // offer (through the system's module, or one picked in Rust's dialog);
  // the PIN goes with the connection and is never stored.
  const needsTokenCert = fields.includes('tokenCertificate');
  const needsPin = fields.includes('tokenPin');
  const [module, setModule] = useState<ModuleChoice | null>(null);
  const [tokenList, setTokenList] = useState<TokenCertList | null>(null);
  const [chosenKey, setChosenKey] = useState<string | null>(null);
  const [tokenError, setTokenError] = useState<string | null>(null);

  const searchTokens = useCallback(async (moduleToken: string | null) => {
    setTokenError(null);
    const result = await commands.listTokenCertificates(moduleToken);
    if (result.status === 'error') {
      setTokenList(null);
      setTokenError(result.error.message);
      return;
    }
    setTokenList(result.data);
    const usable = result.data.certificates.filter((c) => c.certificate !== null);
    setChosenKey(usable.length === 1 ? (usable[0]?.keyId ?? null) : null);
  }, []);

  // On close, erases what was typed.
  useEffect(() => {
    if (!open) {
      setValues({});
      setP12Password('');
      setCertError(null);
      setTokenError(null);
    }
  }, [open]);
  // Opening with a token certificate to choose: look on the tokens at once.
  useEffect(() => {
    if (open && needsTokenCert) void searchTokens(null);
  }, [open, needsTokenCert, searchTokens]);
  useEffect(() => setRemember(keyringAvailable), [keyringAvailable]);

  const ordered = ORDER.filter((f) => fields.includes(f));
  const typedComplete = ordered.every((f) => (values[FIELDS[f].key] ?? '').length > 0);
  const certComplete = !needsCert || (keyringAvailable && p12 !== null && p12Password.length > 0);
  const tokenComplete =
    (!needsTokenCert || (keyringAvailable && chosenKey !== null)) &&
    (!needsPin || (values.tokenPin ?? '').length > 0);
  const complete = typedComplete && certComplete && tokenComplete && !busy;

  async function chooseModule() {
    const result = await commands.chooseTokenModule();
    if (result.status === 'error') setTokenError(result.error.message);
    else if (result.data) {
      setModule(result.data);
      await searchTokens(result.data.token);
    }
  }

  async function chooseP12() {
    const result = await commands.chooseP12File();
    if (result.status === 'error') setCertError(result.error.message);
    else if (result.data) {
      setP12(result.data);
      setCertError(null);
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!complete) return;
    if (needsCert && p12) {
      setBusy(true);
      const result = await commands.importClientCertificate(profileId, p12.token, p12Password);
      setBusy(false);
      setP12Password('');
      if (result.status === 'error') {
        setCertError(result.error.message);
        return;
      }
    }
    if (needsTokenCert && chosenKey) {
      setBusy(true);
      const result = await commands.rememberTokenCertificate(
        profileId,
        module?.token ?? null,
        chosenKey,
      );
      setBusy(false);
      if (result.status === 'error') {
        setTokenError(result.error.message);
        return;
      }
    }
    onSubmit(values, remember && keyringAvailable);
    setValues({});
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={(e) => void submit(e)} className="grid gap-5">
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
            {needsCert && (
              <fieldset className="grid gap-2">
                <legend className="mb-1 text-sm font-medium">{t.certTitle}</legend>
                {keyringAvailable ? (
                  <>
                    <Button
                      type="button"
                      variant="secondary"
                      className="w-fit"
                      onClick={() => void chooseP12()}
                    >
                      <FileKey />
                      {t.certChoose}
                    </Button>
                    {p12 && (
                      <p className="break-all text-xs text-muted-foreground" data-selectable>
                        {t.certChosen(p12.fileName)}
                      </p>
                    )}
                    <Label htmlFor={`${baseId}-p12`}>{t.certPassword}</Label>
                    <SecretInput
                      id={`${baseId}-p12`}
                      value={p12Password}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) => setP12Password(e.target.value)}
                    />
                    <p className="text-xs text-muted-foreground">{t.certHint}</p>
                  </>
                ) : (
                  <p className="text-xs text-destructive">{t.certNeedsKeyring}</p>
                )}
                {certError && (
                  <p role="alert" className="text-xs text-destructive">
                    {certError}
                  </p>
                )}
              </fieldset>
            )}
            {needsTokenCert && (
              <fieldset className="grid gap-2">
                <legend className="mb-1 text-sm font-medium">{t.tokenTitle}</legend>
                {keyringAvailable ? (
                  <>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        type="button"
                        variant="secondary"
                        onClick={() => void searchTokens(module?.token ?? null)}
                      >
                        <Search />
                        {t.tokenSearch}
                      </Button>
                      <Button type="button" variant="ghost" onClick={() => void chooseModule()}>
                        <Usb />
                        {t.tokenOtherModule}
                      </Button>
                    </div>
                    {tokenList && (
                      <p className="break-all text-xs text-muted-foreground" data-selectable>
                        {t.tokenModule(tokenList.moduleName)}
                      </p>
                    )}
                    {tokenList && tokenList.certificates.length === 0 && (
                      <p className="text-xs text-muted-foreground">{t.tokenNone}</p>
                    )}
                    {tokenList && tokenList.certificates.length > 0 && (
                      <div role="radiogroup" aria-label={t.tokenTitle} className="grid gap-2">
                        {tokenList.certificates.map((option) => {
                          const radioId = `${baseId}-token-${option.keyId}`;
                          return (
                            <label
                              key={option.keyId}
                              htmlFor={radioId}
                              className="flex gap-2 rounded-lg border p-2 text-sm has-[:disabled]:opacity-60"
                            >
                              <input
                                id={radioId}
                                type="radio"
                                name={`${baseId}-token`}
                                className="mt-1"
                                value={option.keyId}
                                disabled={option.certificate === null}
                                checked={chosenKey === option.keyId}
                                onChange={() => setChosenKey(option.keyId)}
                              />
                              <span className="grid gap-0.5">
                                <span className="break-all" data-selectable>
                                  {option.certificate?.subject ?? option.keyId}
                                </span>
                                <span className="text-xs text-muted-foreground">
                                  {t.tokenFrom(option.tokenLabel)}
                                  {option.certificate &&
                                    ` · ${option.certificate.key} · ${option.certificate.notAfter}`}
                                </span>
                                {option.problem && (
                                  <span className="text-xs text-destructive">
                                    {t.tokenUnusable(option.problem)}
                                  </span>
                                )}
                              </span>
                            </label>
                          );
                        })}
                      </div>
                    )}
                  </>
                ) : (
                  <p className="text-xs text-destructive">{t.certNeedsKeyring}</p>
                )}
                {tokenError && (
                  <p role="alert" className="text-xs text-destructive">
                    {tokenError}
                  </p>
                )}
              </fieldset>
            )}
            {needsPin && (
              <div className="grid gap-2">
                <Label htmlFor={`${baseId}-pin`}>{t.tokenPin}</Label>
                <SecretInput
                  id={`${baseId}-pin`}
                  value={values.tokenPin ?? ''}
                  autoComplete="off"
                  spellCheck={false}
                  autoFocus={ordered.length === 0 && !needsTokenCert}
                  onChange={(e) => setValues((v) => ({ ...v, tokenPin: e.target.value }))}
                />
                <p className="text-xs text-muted-foreground">{t.tokenPinHint}</p>
                <p className="text-xs text-muted-foreground">{t.tokenKeepOpen}</p>
              </div>
            )}
          </div>

          {ordered.length > 0 && (
            <div className="flex items-start gap-2">
              <Checkbox
                id={`${baseId}-remember`}
                checked={remember && keyringAvailable}
                disabled={!keyringAvailable}
                onCheckedChange={(c) => setRemember(c === true)}
              />
              <div className="grid gap-1">
                <Label htmlFor={`${baseId}-remember`}>{t.remember}</Label>
                {!keyringAvailable && (
                  <p className="text-xs text-muted-foreground">{t.noKeyring}</p>
                )}
                {otp && <p className="text-xs text-muted-foreground">{t.otpHint}</p>}
              </div>
            </div>
          )}

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
