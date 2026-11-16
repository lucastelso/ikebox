import { useCallback, useEffect, useReducer, useRef } from 'react';
import { type CredentialsInput, commands, events } from '@/bindings';
import { connectionReducer, initialState, isBusy } from './connectionReducer';

/** Intervalo da consulta de status (tráfego, tempo, conexões abertas pelo CLI). */
export const STATUS_POLL_MS = 2000;

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

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      if (isBusy(phaseRef.current)) return;
      const result = await commands.status();
      if (!cancelled && result.status === 'ok') {
        dispatch({ type: 'statusPolled', connection: result.data });
      }
    };
    void poll();
    const id = setInterval(() => void poll(), STATUS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
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
