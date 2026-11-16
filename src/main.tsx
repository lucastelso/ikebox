import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { I18nProvider } from './i18n';
import './index.css';
import { applyTheme, readTheme } from './lib/theme';

applyTheme(readTheme());

const root = document.getElementById('root');
if (!root) throw new Error('missing #root element');

createRoot(root).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);
