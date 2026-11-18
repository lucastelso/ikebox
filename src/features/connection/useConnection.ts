import { useCallback, useEffect, useReducer, useRef } from 'react';
import { type CredentialsInput, commands, events } from '@/bindings';
import { connectionReducer, initialState, isBusy } from './connectionReducer';

/** Estado da conexão + ações, ligados aos comandos e eventos do Rust. */
export function useConnection() {
  const [state, dispatch] = useReducer(connectionReducer, initialState);
  const phaseRef = useRef(state.phase);
  phaseRef.current = state.phase;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    events.sessionUpdate
      .listen((event) => dispatch({ type: 'sessionUpdate', update: event.payload }))
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Sem eventos (ex.: testes): o status periódico mantém a tela atualizada.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // The status comes from Rust's one monitor (the same that feeds the tray),
  // as an event while the window is visible. The window reads it itself only
  // when it opens and when it becomes visible again.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const read = async () => {
      if (isBusy(phaseRef.current)) return;
      const result = await commands.status();
      if (!cancelled && result.status === 'ok') {
        dispatch({ type: 'statusPolled', connection: result.data });
      }
    };
    void read();
    events.statusUpdate
      .listen((event) => {
        if (!isBusy(phaseRef.current)) {
          dispatch({ type: 'statusPolled', connection: event.payload.connection });
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // No events (tests): the reads below keep the screen up to date.
      });
    const onVisible = () => {
      if (document.visibilityState === 'visible') void read();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      cancelled = true;
      unlisten?.();
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);

  const connect = useCallback(
    async (profileId: string, credentials: CredentialsInput, remember: boolean) => {
      dispatch({ type: 'connectStarted' });
      const result = await commands.connect(profileId, credentials, remember);
      if (result.status === 'ok') dispatch({ type: 'connectSucceeded', connection: result.data });
      else dispatch({ type: 'connectFailed', error: result.error });
      return result;
    },
    [],
  );

  const disconnect = useCallback(async () => {
    dispatch({ type: 'disconnectStarted' });
    const result = await commands.disconnect();
    if (result.status === 'ok') dispatch({ type: 'disconnected' });
    else dispatch({ type: 'disconnectFailed', error: result.error });
  }, []);

  const dismissError = useCallback(() => dispatch({ type: 'dismissError' }), []);

  return { state, connect, disconnect, dismissError };
}
