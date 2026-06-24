<script>
  import { onMount, onDestroy } from 'svelte';

  let { conn, active, config } = $props();

  let terminalEl;
  let term;
  let fitAddon;
  let resizeObserver;
  let configSent = false;
  let unsubBinary;
  let unsubOpen;

  onMount(async () => {
    const { Terminal } = await import('@xterm/xterm');
    const { FitAddon } = await import('@xterm/addon-fit');
    await import('@xterm/xterm/css/xterm.css');

    term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'block',
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", monospace',
      fontWeight: '400',
      lineHeight: 1.2,
      scrollback: 10000,
      allowTransparency: true,
      theme: {
        background: '#181818',
        foreground: '#e8e3d9',
        cursor: '#d97706',
        cursorAccent: '#181818',
        selectionBackground: '#d9770630',
        black: '#2a2a2a',
        red: '#ef4444',
        green: '#22c55e',
        yellow: '#f59e0b',
        blue: '#60a5fa',
        magenta: '#a78bfa',
        cyan: '#22d3ee',
        white: '#e8e3d9',
        brightBlack: '#4a4a4a',
        brightRed: '#f87171',
        brightGreen: '#4ade80',
        brightYellow: '#fcd34d',
        brightBlue: '#93c5fd',
        brightMagenta: '#c4b5fd',
        brightCyan: '#67e8f9',
        brightWhite: '#f5f0e8',
      },
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(terminalEl);

    // Fit after a tick so layout is complete
    setTimeout(() => {
      fitAddon.fit();
      // Send initial resize + config
      conn.sendResize(term.cols, term.rows);
      if (config && !configSent) {
        conn.sendConfig(config);
        configSent = true;
      }
    }, 50);

    // Terminal input → WebSocket
    term.onData((data) => {
      conn.sendInput(data);
    });

    // Terminal resize → WebSocket
    term.onResize(({ cols, rows }) => {
      conn.sendResize(cols, rows);
    });

    // WebSocket binary → Terminal
    unsubBinary = conn.on('binary', (data) => {
      term.write(data);
    });

    // Handle window resize
    resizeObserver = new ResizeObserver(() => {
      if (active && fitAddon) fitAddon.fit();
    });
    resizeObserver.observe(terminalEl);
  });

  onDestroy(() => {
    if (unsubBinary) unsubBinary();
    if (unsubOpen) unsubOpen();
    if (resizeObserver) resizeObserver.disconnect();
    if (term) term.dispose();
  });

  $effect(() => {
    if (active && fitAddon) {
      setTimeout(() => fitAddon.fit(), 10);
    }
    if (active && term) {
      term.focus();
    }
  });
</script>

<div class="terminal-container" bind:this={terminalEl}></div>

<style>
  .terminal-container {
    flex: 1;
    min-height: 0;
    padding: 4px;
    overflow: hidden;
  }

  .terminal-container :global(.xterm) {
    height: 100%;
  }

  .terminal-container :global(.xterm-viewport) {
    overflow-y: hidden !important;
  }
</style>
