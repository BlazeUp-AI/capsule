<script>
  import Landing from './Landing.svelte';
  import Workspace from './Workspace.svelte';

  let sessionState = $state('landing'); // 'landing' | 'connecting' | 'active'
  let sessionConfig = $state(null);
  let sessionId = $state(null);
  let observalTokens = $state(null);

  function handleStart(config) {
    sessionConfig = config;
    sessionState = 'connecting';
  }

  function handleConnected(id, tokens) {
    sessionId = id;
    observalTokens = tokens;
    sessionState = 'active';
  }

  function handleDisconnected() {
    sessionState = 'landing';
    sessionId = null;
    observalTokens = null;
    sessionConfig = null;
  }
</script>

{#if sessionState === 'landing'}
  <Landing onstart={handleStart} />
{:else}
  <Workspace
    config={sessionConfig}
    {sessionId}
    {observalTokens}
    onconnected={handleConnected}
    ondisconnected={handleDisconnected}
  />
{/if}
