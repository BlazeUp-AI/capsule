<script>
  import Landing from './Landing.svelte';
  import Workspace from './Workspace.svelte';

  const serverConfig = { provider: 'server', agent: 'claude' };
  const shouldAutoStart = localStorage.getItem('capsule:autoStartServer') === '1';

  let sessionState = $state(shouldAutoStart ? 'connecting' : 'landing'); // 'landing' | 'connecting' | 'active'
  let sessionConfig = $state(shouldAutoStart ? serverConfig : null);
  let sessionId = $state(null);
  let observalTokens = $state(null);

  function handleStart(config) {
    if (config.autoStartServer) {
      localStorage.setItem('capsule:autoStartServer', '1');
    } else {
      localStorage.removeItem('capsule:autoStartServer');
    }

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
    sessionConfig = serverConfig;
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
