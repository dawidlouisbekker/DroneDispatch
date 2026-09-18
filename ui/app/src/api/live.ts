// WebSocket client for /v1/live and /v1/orders/live. Browsers can't send headers
// on a WebSocket, so the access token goes in the first message.

export type LiveStatus = 'connecting' | 'open' | 'reconnecting' | 'unauthorized' | 'stopped';

export type LiveOptions<M> = {
  url: string;
  /** The `auth` message to send on open, or `null` when signed out. Called on every (re)connect. */
  authMessage: () => Promise<Record<string, unknown> | null>;
  onMessage: (message: M) => void;
  onStatus?: (status: LiveStatus) => void;
  /** Re-send `auth` this often so the server always holds an unexpired token. */
  reauthIntervalMs?: number;
  /** Injected in tests. */
  createSocket?: (url: string) => WebSocket;
};

/** Close code the services use for a missing or invalid token. */
export const CLOSE_UNAUTHORIZED = 4401;
/** `WebSocket.OPEN`, spelled out so this module doesn't need the global in tests. */
const WS_OPEN = 1;
const MAX_BACKOFF_MS = 30_000;

/** Delay before reconnect attempt `attempt` (0-based): 1 s, 2 s, 4 s … capped at 30 s. */
export const backoffMs = (attempt: number) => Math.min(MAX_BACKOFF_MS, 1000 * 2 ** attempt);

/** `http(s)://…` → `ws(s)://…`. */
export const toWebSocketUrl = (httpUrl: string) => httpUrl.replace(/^http/, 'ws');

export function connectLive<M>(options: LiveOptions<M>): () => void {
  const createSocket = options.createSocket ?? ((url: string) => new WebSocket(url));
  let socket: WebSocket | undefined;
  let attempt = 0;
  let unauthorizedCloses = 0;
  let stopped = false;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let reauthTimer: ReturnType<typeof setInterval> | undefined;

  const setStatus = (status: LiveStatus) => options.onStatus?.(status);

  const sendAuth = async () => {
    const message = await options.authMessage();
    if (!message) {
      stop();
      setStatus('unauthorized');
      return;
    }
    if (socket?.readyState === WS_OPEN) {
      socket.send(JSON.stringify(message));
    }
  };

  const connect = () => {
    setStatus(attempt === 0 ? 'connecting' : 'reconnecting');
    const ws = createSocket(options.url);
    socket = ws;

    ws.onopen = () => {
      void sendAuth();
      reauthTimer = setInterval(() => void sendAuth(), options.reauthIntervalMs ?? 10 * 60_000);
    };
    ws.onmessage = (event) => {
      let message: M & { type?: string };
      try {
        message = JSON.parse(String(event.data));
      } catch {
        return;
      }
      if (message.type === 'ready') {
        attempt = 0;
        unauthorizedCloses = 0;
        setStatus('open');
      }
      options.onMessage(message);
    };
    ws.onclose = (event) => {
      clearInterval(reauthTimer);
      if (stopped) return;
      if (event.code === CLOSE_UNAUTHORIZED && ++unauthorizedCloses >= 2) {
        // The token was refreshed once and still rejected: stop instead of looping.
        stopped = true;
        setStatus('unauthorized');
        return;
      }
      retryTimer = setTimeout(connect, backoffMs(attempt++));
    };
  };

  const stop = () => {
    stopped = true;
    clearTimeout(retryTimer);
    clearInterval(reauthTimer);
    socket?.close();
    setStatus('stopped');
  };

  connect();
  return stop;
}
