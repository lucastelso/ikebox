import { Check, Copy, ScrollText } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { useT } from '@/i18n';

/**
 * charon's log (identities already masked by Rust), collapsible and copyable.
 * The copied text goes through `anonymize` (Rust): the gateway, IPs and
 * domains become placeholders, so the log can go into a bug report.
 */
export function LogPanel({
  lines,
  anonymize,
}: {
  lines: string[];
  /** Returns the anonymized text, or `null` if it failed (nothing is copied). */
  anonymize: (lines: string[]) => Promise<string | null>;
}) {
  const t = useT().log;
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const endRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (lines.length > 0) endRef.current?.scrollIntoView?.({ block: 'end' });
  }, [lines]);

  async function copy() {
    setCopyFailed(false);
    const text = await anonymize(lines);
    if (text === null) {
      // Without anonymizing, nothing goes to the clipboard.
      setCopyFailed(true);
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // No clipboard: the user can still select the text.
    }
  }

  return (
    <details className="group w-full rounded-2xl border bg-card">
      <summary className="flex cursor-pointer select-none items-center gap-2 rounded-2xl px-4 py-3 text-sm font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <ScrollText className="size-4 text-muted-foreground" aria-hidden />
        {t.title}
        <span className="text-xs font-normal text-muted-foreground">{t.lines(lines.length)}</span>
      </summary>
      <div className="border-t">
        <div className="flex items-center justify-between gap-2 px-4 pt-2">
          <p className="text-xs text-muted-foreground">{copyFailed ? t.copyFailed : t.copyHint}</p>
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            onClick={() => void copy()}
            disabled={lines.length === 0}
          >
            {copied ? <Check /> : <Copy />}
            {copied ? t.copied : t.copy}
          </Button>
        </div>
        <pre
          className="max-h-56 overflow-auto whitespace-pre-wrap break-words px-4 pb-4 font-mono text-xs leading-relaxed text-muted-foreground"
          data-selectable
        >
          {lines.length === 0 ? t.empty : lines.join('\n')}
          <span ref={endRef} />
        </pre>
      </div>
    </details>
  );
}
