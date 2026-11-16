/** Tema da interface: segue o sistema por padrão. */
export type Theme = 'system' | 'light' | 'dark';

const KEY = 'ipsec-client:tema';

export function readTheme(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

/** Aplica o tema no <html> (data-theme) e lembra a escolha. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Sem armazenamento: o tema vale só nesta execução.
  }
}
