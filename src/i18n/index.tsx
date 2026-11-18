import { createContext, type ReactNode, useCallback, useContext, useEffect, useState } from 'react';
import { commands } from '@/bindings';
import { en, type Messages, pt } from './messages';

/** Language of the interface. */
export type Language = 'en' | 'pt';
/** The user's choice: follow the system, or a fixed language. */
export type LanguagePref = 'system' | Language;

const KEY = 'ikebox:language';
const MESSAGES: Record<Language, Messages> = { en, pt };

/** Saved choice (default: follow the system). */
export function readLanguagePref(): LanguagePref {
  try {
    const value = localStorage.getItem(KEY);
    return value === 'en' || value === 'pt' ? value : 'system';
  } catch {
    return 'system';
  }
}

function saveLanguagePref(pref: LanguagePref): void {
  try {
    if (pref === 'system') localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, pref);
  } catch {
    // No storage: the choice lasts for this run only.
  }
}

/** System language: Portuguese when the preferred locale is `pt*`, English otherwise. */
export function systemLanguage(): Language {
  const preferred =
    typeof navigator === 'undefined' ? '' : (navigator.languages?.[0] ?? navigator.language ?? '');
  return /^pt\b/i.test(preferred) ? 'pt' : 'en';
}

export function resolveLanguage(pref: LanguagePref): Language {
  return pref === 'system' ? systemLanguage() : pref;
}

interface I18n {
  lang: Language;
  pref: LanguagePref;
  setPref: (pref: LanguagePref) => void;
  t: Messages;
}

// Without a provider (unit tests of a single component): English.
const Context = createContext<I18n>({ lang: 'en', pref: 'en', setPref: () => {}, t: en });

/** Language of the whole app. Tells Rust, so its messages match the UI. */
export function I18nProvider({
  initial,
  children,
}: {
  /** Starting choice (tests); by default, the saved one. */
  initial?: LanguagePref;
  children: ReactNode;
}) {
  const [pref, setPrefState] = useState<LanguagePref>(() => initial ?? readLanguagePref());
  const lang = resolveLanguage(pref);

  useEffect(() => {
    document.documentElement.lang = lang === 'pt' ? 'pt-BR' : 'en';
    commands.setLanguage(lang).catch(() => {
      // Outside Tauri (tests): nothing to tell.
    });
  }, [lang]);

  const setPref = useCallback((next: LanguagePref) => {
    saveLanguagePref(next);
    setPrefState(next);
  }, []);

  return (
    <Context.Provider value={{ lang, pref, setPref, t: MESSAGES[lang] }}>
      {children}
    </Context.Provider>
  );
}

/** Texts in the current language. */
export function useT(): Messages {
  return useContext(Context).t;
}

/** Current language and the user's choice. */
export function useLanguage(): Omit<I18n, 't'> {
  const { lang, pref, setPref } = useContext(Context);
  return { lang, pref, setPref };
}
