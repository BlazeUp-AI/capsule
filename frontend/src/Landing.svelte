<script>
  let { onstart } = $props();

  const PROVIDERS = {
    server: {
      label: 'Server Settings',
      note: 'Uses the runtime environment. Good for the shared OpenRouter or Anthropic configuration.',
      fields: [],
    },
    anthropic_key: {
      label: 'Anthropic Key',
      note: 'Uses a one-session Anthropic key.',
      fields: [{ key: 'ANTHROPIC_API_KEY', label: 'Anthropic API Key', placeholder: 'sk-ant-...' }],
    },
    deepseek_key: {
      label: 'DeepSeek Key',
      note: 'Uses DeepSeek Anthropic compatibility for this session only.',
      fields: [{ key: 'DEEPSEEK_API_KEY', label: 'DeepSeek API Key', placeholder: 'sk-...' }],
    },
    openrouter_key: {
      label: 'OpenRouter Key',
      note: 'Uses OpenRouter Anthropic Skin. Claude Code is most reliable with Anthropic models here.',
      fields: [
        { key: 'OPENROUTER_API_KEY', label: 'OpenRouter API Key', placeholder: 'sk-or-...' },
        { key: 'OPENROUTER_MODEL', label: 'Model', placeholder: '~anthropic/claude-sonnet-latest', secret: false, required: false },
      ],
    },
  };

  let provider = $state('server');
  let credentials = $state({});
  let repo = $state('');
  let observalKey = $state('');
  let autoStartServer = $state(localStorage.getItem('capsule:autoStartServer') === '1');

  function buildCredentials() {
    const key = (credentials[`${provider === 'deepseek_key' ? 'DEEPSEEK' : provider === 'openrouter_key' ? 'OPENROUTER' : 'ANTHROPIC'}_API_KEY`] || '').trim();

    if (provider === 'server') {
      return {};
    }

    if (provider === 'anthropic_key') {
      return { ANTHROPIC_API_KEY: key };
    }

    if (provider === 'deepseek_key') {
      return {
        ANTHROPIC_BASE_URL: 'https://api.deepseek.com/anthropic',
        ANTHROPIC_AUTH_TOKEN: key,
        ANTHROPIC_API_KEY: '',
        ANTHROPIC_MODEL: 'deepseek-v4-pro[1m]',
        ANTHROPIC_DEFAULT_OPUS_MODEL: 'deepseek-v4-pro[1m]',
        ANTHROPIC_DEFAULT_SONNET_MODEL: 'deepseek-v4-pro[1m]',
        ANTHROPIC_DEFAULT_HAIKU_MODEL: 'deepseek-v4-flash',
        CLAUDE_CODE_SUBAGENT_MODEL: 'deepseek-v4-flash',
        CLAUDE_CODE_EFFORT_LEVEL: 'max',
      };
    }

    const model = (credentials.OPENROUTER_MODEL || '').trim() || '~anthropic/claude-sonnet-latest';
    return {
      OPENROUTER_API_KEY: key,
      ANTHROPIC_BASE_URL: 'https://openrouter.ai/api',
      ANTHROPIC_AUTH_TOKEN: key,
      ANTHROPIC_API_KEY: '',
      ANTHROPIC_DEFAULT_OPUS_MODEL: model,
      ANTHROPIC_DEFAULT_SONNET_MODEL: model,
      ANTHROPIC_DEFAULT_HAIKU_MODEL: model,
      CLAUDE_CODE_SUBAGENT_MODEL: model,
    };
  }

  function handleSubmit(e) {
    e.preventDefault();
    const creds = buildCredentials();
    if (observalKey.trim()) {
      creds['OBSERVAL_LICENSE_KEY'] = observalKey.trim();
    }

    onstart({
      provider,
      credentials: Object.keys(creds).length > 0 ? creds : undefined,
      agent: 'claude',
      repo: repo || undefined,
      autoStartServer: provider === 'server' && autoStartServer,
    });
  }

  $effect(() => {
    provider;
    credentials = {};
    if (provider !== 'server') {
      autoStartServer = false;
    }
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

      <div class="provider-note">
        {PROVIDERS[provider].note}
      </div>

      {#each PROVIDERS[provider].fields as field}
        <label class="field">
          <span class="field-label">{field.label}</span>
          <input
            type={field.secret !== false ? 'password' : 'text'}
            placeholder={field.placeholder}
            bind:value={credentials[field.key]}
            required={field.required !== false}
          />
        </label>
      {/each}

      {#if provider === 'server'}
        <label class="check-field">
          <input type="checkbox" bind:checked={autoStartServer} />
          <span>Open terminal immediately next time</span>
        </label>
      {/if}

      <label class="field">
        <span class="field-label">Repository (optional)</span>
        <input type="text" placeholder="https://github.com/..." bind:value={repo} />
      </label>

      <div class="section-divider"></div>

      <label class="field">
        <span class="field-label">Observal License Key (optional)</span>
        <input
          type="password"
          placeholder="eyJ..."
          bind:value={observalKey}
        />
        <span class="field-hint">Enables enterprise telemetry, tracing & insights</span>
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

  .check-field {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-lo);
    font-size: 10px;
    line-height: 1.4;
  }

  .field-label {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-lo);
  }

  .field-hint {
    font-size: 9px;
    color: var(--text-lo);
    opacity: 0.6;
    margin-top: 2px;
  }

  .provider-note {
    font-size: 10px;
    color: var(--text-lo);
    background: var(--bg-inset);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    line-height: 1.5;
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

  input[type='checkbox'] {
    width: 13px;
    height: 13px;
    accent-color: var(--accent);
  }

  .section-divider {
    border-top: 1px solid var(--border);
    margin: 4px 0;
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
