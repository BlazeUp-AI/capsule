<script>
  import { onMount } from 'svelte';

  let { sessionId, active } = $props();
  let loaded = $state(false);
  let iframeEl = $state(null);

  $effect(() => {
    if (active && sessionId && !loaded) {
      loaded = true;
    }
  });

  onMount(() => {
    // Write Observal tokens to sessionStorage for the iframe to pick up
    const token = sessionStorage.getItem('observal_access_token');
    if (!token) {
      // Tokens should already be set by Workspace when session connects
      // If not set, the iframe will show a login page (degraded experience)
    }
  });
</script>

<div class="traces-container">
  {#if loaded && sessionId}
    <iframe
      bind:this={iframeEl}
      src="/observal/{sessionId}/"
      title="Observal Dashboard"
      class="traces-iframe"
    ></iframe>
  {:else}
    <div class="traces-placeholder">
      {#if !sessionId}
        Waiting for session...
      {:else}
        Click to load traces dashboard
      {/if}
    </div>
  {/if}
</div>

<style>
  .traces-container {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .traces-iframe {
    width: 100%;
    height: 100%;
    border: none;
    background: var(--bg-inset);
  }

  .traces-placeholder {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-lo);
    font-size: 11px;
  }
</style>
