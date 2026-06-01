<script>
  let { onstart } = $props();

  const PROVIDERS = {
    anthropic: { label: 'Anthropic', fields: [{ key: 'ANTHROPIC_API_KEY', label: 'API Key', placeholder: 'sk-ant-...' }] },
    bedrock: { label: 'AWS Bedrock (IAM Keys)', fields: [
      { key: 'AWS_ACCESS_KEY_ID', label: 'Access Key ID', placeholder: 'AKIA...' },
      { key: 'AWS_SECRET_ACCESS_KEY', label: 'Secret Access Key', placeholder: '' },
      { key: 'AWS_REGION', label: 'Region', placeholder: 'us-east-1' },
    ]},
    bedrock_bearer: { label: 'AWS Bedrock (Bearer Token)', fields: [
      { key: 'AWS_BEARER_TOKEN_BEDROCK', label: 'Bearer Token', placeholder: 'ABSK...', secret: true },
      { key: 'AWS_REGION', label: 'Region', placeholder: 'us-east-1', secret: false },
    ]},
    openai: { label: 'OpenAI', fields: [{ key: 'OPENAI_API_KEY', label: 'API Key', placeholder: 'sk-...' }] },
    deepseek: { label: 'DeepSeek', fields: [{ key: 'DEEPSEEK_API_KEY', label: 'API Key', placeholder: '' }] },
    gemini: { label: 'Google Gemini', fields: [{ key: 'GOOGLE_API_KEY', label: 'API Key', placeholder: '' }] },
    azure: { label: 'Azure OpenAI', fields: [
      { key: 'AZURE_OPENAI_API_KEY', label: 'API Key', placeholder: '' },
      { key: 'AZURE_OPENAI_ENDPOINT', label: 'Endpoint URL', placeholder: 'https://...' },
      { key: 'AZURE_OPENAI_DEPLOYMENT_NAME', label: 'Deployment Name', placeholder: '' },
    ]},
  };

  let provider = $state('anthropic');
  let credentials = $state({});
  let repo = $state('');

  function handleSubmit(e) {
    e.preventDefault();
    const creds = { ...credentials };
    if (provider === 'bedrock' || provider === 'bedrock_bearer') {
      creds['CLAUDE_CODE_USE_BEDROCK'] = '1';
    }
    onstart({
      provider,
      credentials: creds,
      agent: 'claude',
      repo: repo || undefined,
    });
  }

  $effect(() => {
    credentials = {};
  });
</script>

<div class="landing">
  <div class="landing-card">
    <div class="logo">
      <svg viewBox="0 0 18 18" fill="none" width="24" height="24">
        <rect x="1" y="5" width="16" height="8" rx="4" stroke="#d97706" stroke-width="1" fill="none" opacity="0.8"/>
        <line x1="9" y1="5" x2="9" y2="13" stroke="#d97706" stroke-width="1" opacity="0.5"/>
        <rect x="1" y="5" width="8" height="8" rx="4" fill="#d9770618"/>
      </svg>
      <span class="title">CAPSULE</span>
    </div>

    <form onsubmit={handleSubmit}>
      <label class="field">
        <span class="field-label">Provider</span>
        <select bind:value={provider}>
          {#each Object.entries(PROVIDERS) as [key, p]}
            <option value={key}>{p.label}</option>
          {/each}
        </select>
      </label>

      {#each PROVIDERS[provider].fields as field}
        <label class="field">
          <span class="field-label">{field.label}</span>
          <input
            type={field.secret !== false ? 'password' : 'text'}
            placeholder={field.placeholder}
            bind:value={credentials[field.key]}
            required
          />
        </label>
      {/each}

      <label class="field">
        <span class="field-label">Repository (optional)</span>
        <input type="text" placeholder="https://github.com/..." bind:value={repo} />
      </label>

      <button type="submit" class="btn-start">Start Session</button>
    </form>
  </div>
</div>

<style>
  .landing {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .landing-card {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 32px;
    width: 100%;
    max-width: 400px;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 24px;
  }

  .title {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-hi);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .field-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-lo);
  }

  input, select {
    background: var(--bg-inset);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-family: var(--font);
    font-size: 12px;
    padding: 8px 10px;
    outline: none;
  }

  input:focus, select:focus {
    border-color: var(--accent);
  }

  .btn-start {
    margin-top: 8px;
    background: var(--accent);
    color: var(--bg);
    border: none;
    border-radius: var(--radius);
    font-family: var(--font);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 10px;
    cursor: pointer;
  }

  .btn-start:hover {
    background: var(--amber);
  }
</style>
