<script>
  let { sessionId, active } = $props();
  let loaded = $state(false);
  let iframeEl = $state(null);

  $effect(() => {
    if (active && sessionId && !loaded) {
      loaded = true;
    }
  });
</script>

<div class="traces-container">
  {#if loaded && sessionId}
    <iframe
      bind:this={iframeEl}
      src="/observal/{sessionId}/sessions"
      title="Observal Dashboard"
      class="traces-iframe"
    ></iframe>
  {:else}
    <div class="traces-placeholder">
      {#if !sessionId}
        Waiting for session...
      {:else}
        Click Traces tab to load dashboard
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
