/** Interface theme: follows the system by default. */
export type Theme = 'system' | 'light' | 'dark';

const KEY = 'ikebox:tema';

export function readTheme(): Theme {
  try {
    const value = localStorage.getItem(KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

/** Applies the theme to <html> (data-theme) and remembers the choice. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // No storage: the theme applies to this run only.
  }
}
