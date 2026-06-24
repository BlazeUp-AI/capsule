/**
 * Shared WebSocket connection for the Capsule workspace.
 * Handles connection lifecycle, message routing, and reconnection.
 */

export function createConnection() {
  let ws = null;
  let listeners = new Map();
  let sessionId = null;

  function connect(config) {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/ws`;

    ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => {
      emit('open');
      // Send resize first (required by backend)
      // Terminal component will call sendResize() after mounting
    };

    ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        emit('binary', new Uint8Array(e.data));
        return;
      }
      const msg = JSON.parse(e.data);
      emit('message', msg);

      if (msg.type === 'connected') {
        sessionId = msg.session_id;
        emit('connected', msg);
      } else if (msg.type === 'reconnected') {
        sessionId = msg.session_id;
        emit('reconnected', msg);
      } else if (msg.type === 'session_state') {
        emit('session_state', msg);
      } else if (msg.type === 'error') {
        emit('error', msg);
      }
    };

    ws.onclose = (e) => {
      emit('close', e);
    };

    ws.onerror = () => {
      emit('error', { message: 'WebSocket error' });
    };
  }

  function sendJson(obj) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(obj));
    }
  }

  function sendResize(cols, rows) {
    sendJson({ type: 'resize', cols, rows });
  }

  function sendConfig(config) {
    sendJson({
      type: 'session_config',
      credentials: config.credentials,
      agent: config.agent,
      image: config.image,
      repo: config.repo,
      enable_dind: config.enable_dind,
    });
  }

  function sendInput(data) {
    sendJson({ type: 'terminal_input', data });
  }

  function close() {
    if (ws) {
      ws.close();
      ws = null;
    }
  }

  function on(event, fn) {
    if (!listeners.has(event)) listeners.set(event, []);
    listeners.get(event).push(fn);
    return () => {
      const fns = listeners.get(event);
      if (fns) listeners.set(event, fns.filter(f => f !== fn));
    };
  }

  function emit(event, data) {
    const fns = listeners.get(event);
    if (fns) fns.forEach(fn => fn(data));
  }

  return {
    connect,
    sendResize,
    sendConfig,
    sendInput,
    close,
    on,
    get sessionId() { return sessionId; },
    get isOpen() { return ws && ws.readyState === WebSocket.OPEN; },
  };
}
