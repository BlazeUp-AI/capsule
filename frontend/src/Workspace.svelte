<script>
  import { onMount, onDestroy } from 'svelte';
  import { createConnection } from './ws.js';
  import Terminal from './Terminal.svelte';
  import Editor from './Editor.svelte';
  import Traces from './Traces.svelte';

  let { config, onconnected, ondisconnected } = $props();

  let activeTab = $state('terminal');
  let connState = $state('connecting'); // 'connecting' | 'provisioning' | 'ready' | 'error'
  let stateDetail = $state('connecting to runtime...');
  let localSessionId = $state(null);

  const conn = createConnection();

  onMount(() => {
    conn.on('open', () => {
      connState = 'connecting';
      stateDetail = 'connected, waiting for session...';
    });

    conn.on('session_state', (msg) => {
      connState = 'provisioning';
      stateDetail = msg.detail;
    });

    conn.on('connected', (msg) => {
      connState = 'ready';
      localSessionId = msg.session_id;
      // Write Observal tokens to sessionStorage for the iframe to pick up
      if (msg.observal_token) {
        sessionStorage.setItem('observal_access_token', msg.observal_token);
      }
      if (msg.observal_refresh_token) {
        sessionStorage.setItem('observal_refresh_token', msg.observal_refresh_token);
      }
      onconnected(msg.session_id, null);
    });

    conn.on('reconnected', (msg) => {
      connState = 'ready';
      localSessionId = msg.session_id;
    });

    conn.on('error', (msg) => {
      connState = 'error';
      stateDetail = msg.message || 'connection error';
    });

    conn.on('close', () => {
      if (connState === 'ready') {
        ondisconnected();
      } else {
        connState = 'error';
        stateDetail = 'connection lost';
      }
    });

    conn.connect(config);
  });

  onDestroy(() => {
    conn.close();
  });

  function switchTab(tab) {
    activeTab = tab;
  }

  function handleKeydown(e) {
    if (e.ctrlKey && e.key >= '1' && e.key <= '3') {
      e.preventDefault();
      const tabs = ['terminal', 'editor', 'traces'];
      switchTab(tabs[parseInt(e.key) - 1]);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="workspace">
  <!-- Top bar -->
  <div class="topbar">
    <div class="tabs">
      <button class="tab" class:active={activeTab === 'terminal'} onclick={() => switchTab('terminal')}>
        Terminal
      </button>
      <button class="tab" class:active={activeTab === 'editor'} onclick={() => switchTab('editor')}>
        Editor
      </button>
      <button class="tab" class:active={activeTab === 'traces'} onclick={() => switchTab('traces')}>
        Traces
      </button>
    </div>

    <div class="status">
      <span class="session-id">{localSessionId ? localSessionId.slice(0, 8) : '—'}</span>
      <span class="conn-dot" class:connected={connState === 'ready'} class:connecting={connState === 'connecting' || connState === 'provisioning'} class:error={connState === 'error'}></span>
    </div>
  </div>

  <!-- Tab content -->
  <div class="content">
    {#if connState !== 'ready'}
      <div class="overlay">
        {#if connState === 'error'}
          <div class="overlay-title">Connection Error</div>
          <div class="overlay-sub">{stateDetail}</div>
        {:else}
          <div class="spinner"></div>
          <div class="overlay-title">{stateDetail}</div>
        {/if}
      </div>
    {/if}

    <div class="tab-panel" class:visible={activeTab === 'terminal'}>
      <Terminal {conn} active={activeTab === 'terminal' && connState === 'ready'} {config} />
    </div>

    <div class="tab-panel" class:visible={activeTab === 'editor'}>
      <Editor sessionId={localSessionId} active={activeTab === 'editor' && connState === 'ready'} />
    </div>

    <div class="tab-panel" class:visible={activeTab === 'traces'}>
      <Traces sessionId={localSessionId} active={activeTab === 'traces' && connState === 'ready'} />
    </div>
  </div>
</div>

<style>
  .workspace {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 8px;
    gap: 6px;
  }

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0 12px;
    height: 36px;
    flex-shrink: 0;
  }

  .tabs {
    display: flex;
    gap: 0;
  }

  .tab {
    background: none;
    border: none;
    color: var(--text-lo);
    font-family: var(--font);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 8px 14px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
  }

  .tab:hover { color: var(--text); }
  .tab.active { color: var(--accent); border-bottom-color: var(--accent); }

  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10px;
    color: var(--text-lo);
  }

  .session-id {
    font-variant-numeric: tabular-nums;
  }

  .conn-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-lo);
  }

  .conn-dot.connected { background: var(--accent); box-shadow: 0 0 6px var(--accent); }
  .conn-dot.connecting { background: var(--amber); animation: pulse 1s ease-in-out infinite; }
  .conn-dot.error { background: var(--red); }

  @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }

  .content {
    flex: 1;
    position: relative;
    min-height: 0;
    background: var(--bg-inset);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .tab-panel {
    position: absolute;
    inset: 0;
    display: none;
  }

  .tab-panel.visible {
    display: flex;
    flex-direction: column;
  }

  .overlay {
    position: absolute;
    inset: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    background: var(--bg-inset);
  }

  .overlay-title {
    font-size: 11px;
    letter-spacing: 0.1em;
    color: var(--text);
  }

  .overlay-sub {
    font-size: 10px;
    color: var(--text-lo);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 1px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }
</style>
