import { ArrowLeft, FileUp, Info, ShieldCheck, X } from 'lucide-react';
import { type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { toast } from 'sonner';
import {
  commands,
  type DraftAuth,
  type DraftCheck,
  type ProfileDraft,
  type ProfileSummary,
  type ProposalPresetInfo,
  type Proposals,
} from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useT } from '@/i18n';
import { cn } from '@/lib/utils';
import { SecurityBadge, SecurityNotes } from './SecurityBadge';

type ProposalMode = Proposals | 'advanced';
type ListField = 'ikeProposals' | 'espProposals' | 'remoteTs' | 'dnsServers' | 'dnsDomains';

const sameList = (a: string[], b: string[]) =>
  a.length === b.length && a.every((v, i) => v === b[i]);

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="grid gap-3">
      <legend className="mb-1 text-sm font-semibold">{title}</legend>
      {children}
    </fieldset>
  );
}

/** A label, the control, a hint and the problems of the field. */
function Field({
  id,
  label,
  hint,
  issues,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  issues: string[];
  children: ReactNode;
}) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && issues.length === 0 && (
        <p id={`${id}-hint`} className="text-xs text-muted-foreground">
          {hint}
        </p>
      )}
      {issues.map((issue) => (
        <p key={issue} id={`${id}-error`} className="text-xs text-destructive">
          {issue}
        </p>
      ))}
    </div>
  );
}

function Toggle({
  id,
  label,
  hint,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-2">
      <Checkbox id={id} checked={checked} onCheckedChange={(c) => onChange(c === true)} />
      <div className="grid gap-0.5">
        <Label htmlFor={id}>{label}</Label>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </div>
    </div>
  );
}

const TEXTAREA =
  'min-h-[4.5rem] w-full rounded-lg border border-input bg-card px-3 py-2 font-mono text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring aria-invalid:border-destructive';

/**
 * Profile editor (phase 7). The draft goes to Rust as the person types: Rust
 * says what is wrong, how safe it is and which proposal preset it is. Nothing
 * is validated only here.
 */
export function ProfileEditor({
  initial,
  onSaved,
  onCancel,
}: {
  initial: ProfileDraft;
  onSaved: (profile: ProfileSummary) => void;
  onCancel: () => void;
}) {
  const t = useT().editor;
  const base = useId();
  const [draft, setDraft] = useState(initial);
  const [check, setCheck] = useState<DraftCheck | null>(null);
  const [presets, setPresets] = useState<ProposalPresetInfo[]>([]);
  const [mode, setMode] = useState<ProposalMode | null>(null);
  const modeRef = useRef<ProposalMode | null>(null);
  const [touched, setTouched] = useState<Set<string>>(new Set());
  const [attempted, setAttempted] = useState(false);
  const [saving, setSaving] = useState(false);
  const [pastedCa, setPastedCa] = useState('');
  const editing = draft.originalId !== null;

  // Rust checks the draft a moment after the last change.
  useEffect(() => {
    const timer = setTimeout(async () => {
      const result = await commands.checkProfileDraft(draft);
      if (result.status === 'ok') setCheck(result.data);
    }, 200);
    return () => clearTimeout(timer);
  }, [draft]);

  // Presets of the IKE version; the first time, the mode of the profile.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only the version changes the presets; the proposals are read the first time
  useEffect(() => {
    let cancelled = false;
    void commands.proposalPresets(draft.ikeVersion).then((list) => {
      if (cancelled) return;
      setPresets(list);
      const current = modeRef.current;
      let next: ProposalMode;
      if (current === null) {
        const found = list.find(
          (p) => sameList(p.ike, draft.ikeProposals) && sameList(p.esp, draft.espProposals),
        );
        next = found?.preset ?? 'advanced';
      } else {
        // A preset this version does not have falls back to Compatible; the
        // proposals of a preset change with the version.
        next =
          current !== 'advanced' && !list.some((p) => p.preset === current)
            ? 'compatible'
            : current;
        const preset = list.find((p) => p.preset === next);
        if (preset) {
          setDraft((d) => ({ ...d, ikeProposals: preset.ike, espProposals: preset.esp }));
        }
      }
      modeRef.current = next;
      setMode(next);
    });
    return () => {
      cancelled = true;
    };
  }, [draft.ikeVersion]);

  function set<K extends keyof ProfileDraft>(key: K, value: ProfileDraft[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
  }

  function touch(field: string) {
    setTouched((s) => (s.has(field) ? s : new Set(s).add(field)));
  }

  const issuesFor = (field: string) =>
    attempted || touched.has(field)
      ? (check?.issues.filter((i) => i.field === field).map((i) => i.message) ?? [])
      : [];

  function choosePreset(value: ProposalMode) {
    modeRef.current = value;
    setMode(value);
    const preset = presets.find((p) => p.preset === value);
    if (preset) setDraft((d) => ({ ...d, ikeProposals: preset.ike, espProposals: preset.esp }));
  }

  function setVersion(version: number) {
    setDraft((d) => ({
      ...d,
      ikeVersion: version,
      // Aggressive mode and XAuth only exist in IKEv1, EAP only in IKEv2; a
      // certificate in IKEv1 needs main mode.
      ...(version === 2
        ? { aggressive: false, xauth: false, otp: false }
        : d.auth === 'eap'
          ? { auth: 'psk' as const, ca: [] }
          : d.auth === 'certificate'
            ? { aggressive: false }
            : {}),
    }));
  }

  function setAuth(auth: DraftAuth) {
    setDraft((d) => ({
      ...d,
      auth,
      // EAP has no XAuth after it; the gateway CAs only apply with a certificate.
      ...(auth === 'eap' ? { xauth: false, otp: false } : {}),
      ...(auth === 'psk' ? { ca: [] } : {}),
      ...(auth === 'certificate' ? { aggressive: false } : {}),
    }));
  }

  /** Adds CA entries that are not there yet (Rust splits and later checks them). */
  function addCas(entries: string[]) {
    setDraft((d) => ({ ...d, ca: [...d.ca, ...entries.filter((e) => !d.ca.includes(e))] }));
    touch('ca');
  }

  async function chooseCaFile() {
    const result = await commands.chooseCaFile();
    if (result.status === 'error') toast.error(result.error.message);
    else if (result.data) addCas(result.data);
  }

  async function addPastedCa() {
    const result = await commands.caEntries(pastedCa);
    if (result.status === 'error') toast.error(result.error.message);
    else {
      addCas(result.data);
      setPastedCa('');
    }
  }

  async function save() {
    setAttempted(true);
    if (!check || check.issues.length > 0) return;
    setSaving(true);
    const result = await commands.saveProfileDraft(draft);
    setSaving(false);
    if (result.status === 'ok') {
      toast.success(t.saved);
      onSaved(result.data);
    } else toast.error(result.error.message);
  }

  const id = (name: string) => `${base}-${name}`;
  const text = (name: 'id' | 'name' | 'gateway' | 'localId' | 'remoteId', placeholder?: string) => (
    <Input
      id={id(name)}
      value={draft[name]}
      placeholder={placeholder}
      onChange={(e) => set(name, e.target.value)}
      onBlur={() => touch(name)}
      aria-invalid={issuesFor(name).length > 0}
      disabled={name === 'id' && editing}
      spellCheck={false}
      autoComplete="off"
    />
  );
  const lines = (name: ListField) => (
    <textarea
      id={id(name)}
      className={TEXTAREA}
      value={draft[name].join('\n')}
      onChange={(e) => set(name, e.target.value.split('\n'))}
      onBlur={() => touch(name)}
      aria-invalid={issuesFor(name).length > 0}
      spellCheck={false}
      autoComplete="off"
      rows={Math.max(2, draft[name].length)}
    />
  );
  const general = issuesFor('general');
  // With EAP or a client certificate, the gateway is proven by its own.
  const gatewayCert = draft.auth !== 'psk';
  const presetInfo = presets.find((p) => p.preset === mode);
  const blocked = attempted && (check?.issues.length ?? 0) > 0;

  return (
    <div className="flex w-full flex-col gap-5">
      <div className="flex items-center gap-2">
        <Button size="icon" variant="ghost" onClick={onCancel} aria-label={t.back}>
          <ArrowLeft />
        </Button>
        <h2 className="min-w-0 truncate text-lg font-semibold">
          {editing ? t.editTitle(initial.name) : t.newTitle}
        </h2>
      </div>

      <Section title={t.section.general}>
        <Field id={id('name')} label={t.name} issues={issuesFor('name')}>
          {text('name')}
        </Field>
        <Field id={id('id')} label={t.id} hint={t.idHint} issues={issuesFor('id')}>
          {text('id')}
        </Field>
        <Field id={id('gateway')} label={t.gateway} issues={issuesFor('gateway')}>
          {text('gateway', t.gatewayPlaceholder)}
        </Field>
      </Section>

      <Section title={t.section.auth}>
        <div className="grid grid-cols-2 gap-3">
          <Field id={id('ikeVersion')} label={t.ikeVersion} issues={issuesFor('ikeVersion')}>
            <Select value={String(draft.ikeVersion)} onValueChange={(v) => setVersion(Number(v))}>
              <SelectTrigger id={id('ikeVersion')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="2">IKEv2</SelectItem>
                <SelectItem value="1">IKEv1</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          {draft.ikeVersion === 1 && (
            <Field id={id('mode')} label={t.mode} issues={issuesFor('aggressive')}>
              <Select
                value={draft.aggressive ? 'aggressive' : 'main'}
                onValueChange={(v) => set('aggressive', v === 'aggressive')}
              >
                <SelectTrigger id={id('mode')}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="main">{t.modeMain}</SelectItem>
                  <SelectItem value="aggressive">{t.modeAggressive}</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          )}
        </div>
        <Field id={id('authMethod')} label={t.authMethod} issues={[]}>
          <Select value={draft.auth} onValueChange={(v) => setAuth(v as DraftAuth)}>
            <SelectTrigger id={id('authMethod')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="psk">{t.authPsk}</SelectItem>
              {draft.ikeVersion === 2 && <SelectItem value="eap">{t.authEap}</SelectItem>}
              <SelectItem value="certificate">{t.authCert}</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        {draft.auth === 'certificate' && (
          <>
            <Toggle
              id={id('token')}
              label={t.certOnToken}
              hint={t.certOnTokenHint}
              checked={draft.token === true}
              onChange={(v) => setDraft((d) => ({ ...d, token: v }))}
            />
            <p className="flex gap-1.5 text-xs text-muted-foreground">
              <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden />
              {t.certNote}
            </p>
          </>
        )}
        <Field
          id={id('localId')}
          label={draft.auth === 'psk' ? t.localId : t.localIdEap}
          hint={
            draft.auth === 'psk'
              ? t.localIdHint
              : draft.auth === 'eap'
                ? t.localIdEapHint
                : t.localIdCertHint
          }
          issues={issuesFor('localId')}
        >
          {text('localId', draft.auth === 'psk' ? 'keyid:…' : undefined)}
        </Field>
        {draft.ikeVersion === 1 && (
          <>
            <Toggle
              id={id('xauth')}
              label={t.xauth}
              hint={t.xauthHint}
              checked={draft.xauth}
              onChange={(v) => setDraft((d) => ({ ...d, xauth: v, otp: v && d.otp }))}
            />
            {draft.xauth && (
              <div className="pl-6">
                <Toggle
                  id={id('otp')}
                  label={t.otp}
                  hint={t.otpHint}
                  checked={draft.otp}
                  onChange={(v) => set('otp', v)}
                />
              </div>
            )}
          </>
        )}
        {issuesFor('auth').map((issue) => (
          <p key={issue} className="text-xs text-destructive">
            {issue}
          </p>
        ))}
        <Field
          id={id('remoteId')}
          label={gatewayCert ? t.remoteIdCert : t.remoteId}
          hint={gatewayCert ? t.remoteIdCertHint : t.remoteIdHint}
          issues={issuesFor('remoteId')}
        >
          {text('remoteId', gatewayCert ? t.gatewayPlaceholder : undefined)}
        </Field>
        {gatewayCert && (
          <Field id={id('ca')} label={t.ca} hint={t.caHint} issues={issuesFor('ca')}>
            {draft.ca.length > 0 && (
              <ul className="grid gap-2">
                {draft.ca.map((pem, i) => {
                  const summary = check?.cas[i];
                  return (
                    <li
                      key={pem}
                      className="flex items-start gap-2 rounded-lg border bg-card p-2.5 text-xs"
                    >
                      <ShieldCheck
                        className={cn(
                          'mt-0.5 size-4 shrink-0',
                          summary ? 'text-success' : 'text-destructive',
                        )}
                        aria-hidden
                      />
                      <span className="min-w-0 flex-1 break-words" data-selectable>
                        {summary ? (
                          <>
                            <span className="block font-medium">{summary.subject}</span>
                            <span className="block text-muted-foreground">
                              {summary.key} · {t.caValidUntil(summary.notAfter)}
                            </span>
                          </>
                        ) : (
                          <span className="text-destructive">{t.caUnusable}</span>
                        )}
                      </span>
                      <Button
                        size="icon"
                        variant="ghost"
                        className="size-7"
                        aria-label={t.caRemove(i + 1)}
                        onClick={() =>
                          set(
                            'ca',
                            draft.ca.filter((_, j) => j !== i),
                          )
                        }
                      >
                        <X />
                      </Button>
                    </li>
                  );
                })}
              </ul>
            )}
            <Button variant="secondary" className="w-fit" onClick={() => void chooseCaFile()}>
              <FileUp />
              {t.caChooseFile}
            </Button>
            <p id={id('caPasteHint')} className="mt-1 text-xs text-muted-foreground">
              {t.caPaste}
            </p>
            <textarea
              id={id('ca')}
              aria-describedby={id('caPasteHint')}
              className={TEXTAREA}
              value={pastedCa}
              onChange={(e) => setPastedCa(e.target.value)}
              placeholder="-----BEGIN CERTIFICATE-----"
              spellCheck={false}
              rows={3}
            />
            <Button
              variant="secondary"
              className="w-fit"
              disabled={!pastedCa.trim()}
              onClick={() => void addPastedCa()}
            >
              {t.caAdd}
            </Button>
          </Field>
        )}
      </Section>

      <Section title={t.section.crypto}>
        <Field id={id('preset')} label={t.proposals} issues={[]}>
          <Select value={mode ?? ''} onValueChange={(v) => choosePreset(v as ProposalMode)}>
            <SelectTrigger id={id('preset')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {presets.map((p) => (
                <SelectItem key={p.preset} value={p.preset}>
                  {t.preset[p.preset]}
                </SelectItem>
              ))}
              <SelectItem value="advanced">{t.preset.advanced}</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        {presetInfo && (
          <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
            <span>{t.presetHint[presetInfo.preset]}</span>
            <SecurityBadge level={presetInfo.security} />
          </div>
        )}
        {mode === 'advanced' && (
          <>
            <p className="text-xs text-muted-foreground">{t.presetHint.advanced}</p>
            <Field id={id('ikeProposals')} label={t.phase1} issues={issuesFor('ikeProposals')}>
              {lines('ikeProposals')}
            </Field>
            <Field
              id={id('espProposals')}
              label={t.phase2}
              hint={t.proposalsHint}
              issues={issuesFor('espProposals')}
            >
              {lines('espProposals')}
            </Field>
          </>
        )}
      </Section>

      <Section title={t.section.networks}>
        <Field
          id={id('remoteTs')}
          label={t.remoteTs}
          hint={t.remoteTsHint}
          issues={issuesFor('remoteTs')}
        >
          {lines('remoteTs')}
        </Field>
        <Toggle
          id={id('vip')}
          label={t.requestVip}
          checked={draft.requestVip}
          onChange={(v) => set('requestVip', v)}
        />
      </Section>

      <Section title={t.section.dns}>
        <Field
          id={id('dnsServers')}
          label={t.dnsServers}
          hint={t.dnsServersHint}
          issues={issuesFor('dnsServers')}
        >
          {lines('dnsServers')}
        </Field>
        <Field
          id={id('dnsDomains')}
          label={t.dnsDomains}
          hint={t.dnsDomainsHint}
          issues={issuesFor('dnsDomains')}
        >
          {lines('dnsDomains')}
        </Field>
        <Toggle
          id={id('useReceived')}
          label={t.useReceivedDns}
          checked={draft.useReceivedDns}
          onChange={(v) => set('useReceivedDns', v)}
        />
        <Toggle
          id={id('defaultRoute')}
          label={t.dnsDefaultRoute}
          checked={draft.dnsDefaultRoute}
          onChange={(v) => set('dnsDefaultRoute', v)}
        />
      </Section>

      <details className="group">
        <summary className="cursor-pointer rounded-lg text-sm font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {t.section.advanced}
        </summary>
        <div className="mt-3 grid gap-3">
          <Toggle
            id={id('encap')}
            label={t.encap}
            checked={draft.encap}
            onChange={(v) => set('encap', v)}
          />
          <div className="grid grid-cols-2 gap-3">
            <Field id={id('dpd')} label={t.dpd} hint={t.dpdHint} issues={issuesFor('dpdDelaySecs')}>
              <Input
                id={id('dpd')}
                inputMode="numeric"
                value={draft.dpdDelaySecs ?? ''}
                onChange={(e) => {
                  const v = e.target.value.replace(/\D/g, '');
                  set('dpdDelaySecs', v === '' ? null : Number(v));
                }}
              />
            </Field>
            <Field id={id('mtu')} label={t.mtu} issues={issuesFor('mtu')}>
              <Input
                id={id('mtu')}
                inputMode="numeric"
                value={draft.mtu}
                onChange={(e) => set('mtu', Number(e.target.value.replace(/\D/g, '')) || 0)}
              />
            </Field>
          </div>
        </div>
      </details>

      {general.map((issue) => (
        <p key={issue} className="text-xs text-destructive">
          {issue}
        </p>
      ))}

      {check && check.notes.length > 0 && (
        <Card className="w-full">
          <SecurityNotes notes={check.notes} />
        </Card>
      )}
      {check?.schema === 2 && (
        <p className="flex gap-1.5 text-xs text-muted-foreground">
          <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden />
          {t.schema2}
        </p>
      )}

      <div
        className={cn(
          'sticky bottom-0 -mx-5 flex items-center gap-2 border-t bg-background px-5 py-3',
        )}
      >
        {check && <SecurityBadge level={check.security} />}
        {blocked && (
          <span role="alert" className="min-w-0 text-xs text-destructive">
            {t.problems}
          </span>
        )}
        <div className="ml-auto flex gap-2">
          <Button variant="ghost" onClick={onCancel}>
            {t.cancel}
          </Button>
          <Button onClick={() => void save()} disabled={saving || !check}>
            {t.save}
          </Button>
        </div>
      </div>
    </div>
  );
}
