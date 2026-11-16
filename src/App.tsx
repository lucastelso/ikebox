import { HomeScreen } from './features/connection/HomeScreen';

/** Janela principal. */
export function App() {
  return (
    <div className="flex min-h-full flex-col">
      <header className="flex items-center gap-3 border-b px-5 py-3">
        <img src="/logo.svg" alt="" className="size-7" />
        <h1 className="text-base font-semibold tracking-tight">IPsec Client</h1>
      </header>
      <main className="mx-auto flex w-full max-w-md flex-1 flex-col items-center gap-6 p-5">
        <HomeScreen />
      </main>
    </div>
  );
}
