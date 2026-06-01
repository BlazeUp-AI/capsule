<script>
  import { onMount } from 'svelte';

  let { sessionId, active } = $props();
  let editorEl = $state(null);
  let editor;
  let files = $state([]);
  let currentFile = $state(null);
  let loading = $state(false);

  onMount(() => {
    if (active && sessionId) {
      loadFileTree('/workspace');
    }
  });

  $effect(() => {
    if (active && sessionId) {
      loadFileTree('/workspace');
    }
  });

  async function loadFileTree(path) {
    if (!sessionId) return;
    loading = true;
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files?path=${encodeURIComponent(path)}`);
      if (resp.ok) {
        files = await resp.json();
      }
    } catch (e) {
      // File API not available yet
    }
    loading = false;
  }

  async function openFile(file) {
    if (!sessionId || file.type !== 'f') return;
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(file.path)}`);
      if (resp.ok) {
        const content = await resp.text();
        currentFile = { ...file, content };
        await initEditor(content, file.name);
      }
    } catch (e) {
      // ignore
    }
  }

  async function initEditor(content, filename) {
    if (!editorEl) return;

    if (!editor) {
      const monaco = await import('monaco-editor');
      editor = monaco.editor.create(editorEl, {
        value: content,
        theme: 'vs-dark',
        fontFamily: '"JetBrains Mono", monospace',
        fontSize: 13,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
      });

      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        saveFile();
      });
    } else {
      editor.setValue(content);
    }

    // Set language from extension
    const ext = filename.split('.').pop();
    const langMap = { js: 'javascript', ts: 'typescript', py: 'python', rs: 'rust', go: 'go', md: 'markdown', json: 'json', html: 'html', css: 'css', yml: 'yaml', yaml: 'yaml', toml: 'toml', sh: 'shell' };
    const lang = langMap[ext] || 'plaintext';
    const monaco = await import('monaco-editor');
    monaco.editor.setModelLanguage(editor.getModel(), lang);
  }

  async function saveFile() {
    if (!currentFile || !editor || !sessionId) return;
    const content = editor.getValue();
    await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(currentFile.path)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'text/plain' },
      body: content,
    });
  }
</script>

<div class="editor-layout">
  <div class="file-tree">
    <div class="tree-header">FILES</div>
    {#if loading}
      <div class="tree-loading">loading...</div>
    {:else if files.length === 0}
      <div class="tree-empty">No files yet</div>
    {:else}
      {#each files as file}
        <button
          class="tree-item"
          class:active={currentFile?.path === file.path}
          onclick={() => openFile(file)}
        >
          <span class="tree-icon">{file.type === 'd' ? '/' : ''}</span>
          <span class="tree-name">{file.name}</span>
        </button>
      {/each}
    {/if}
  </div>

  <div class="editor-pane">
    {#if currentFile}
      <div class="editor-tab-bar">
        <span class="editor-tab active">{currentFile.name}</span>
      </div>
      <div class="editor-container" bind:this={editorEl}></div>
    {:else}
      <div class="editor-placeholder">Select a file to edit</div>
    {/if}
  </div>
</div>

<style>
  .editor-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .file-tree {
    width: 200px;
    border-right: 1px solid var(--border);
    overflow-y: auto;
    padding: 8px 0;
    flex-shrink: 0;
  }

  .tree-header {
    font-size: 9px;
    letter-spacing: 0.12em;
    color: var(--text-lo);
    padding: 4px 12px 8px;
  }

  .tree-loading, .tree-empty {
    font-size: 10px;
    color: var(--text-lo);
    padding: 4px 12px;
  }

  .tree-item {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    background: none;
    border: none;
    padding: 3px 12px;
    font-family: var(--font);
    font-size: 11px;
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }

  .tree-item:hover { background: var(--bg-panel); }
  .tree-item.active { background: var(--accent-lo); color: var(--accent); }

  .tree-icon {
    color: var(--text-lo);
    font-size: 10px;
    width: 10px;
  }

  .editor-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .editor-tab-bar {
    display: flex;
    border-bottom: 1px solid var(--border);
    height: 28px;
    align-items: center;
    padding: 0 8px;
  }

  .editor-tab {
    font-size: 10px;
    color: var(--text-lo);
    padding: 4px 10px;
  }

  .editor-tab.active {
    color: var(--text);
    border-bottom: 1px solid var(--accent);
  }

  .editor-container {
    flex: 1;
    min-height: 0;
  }

  .editor-placeholder {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-lo);
    font-size: 11px;
  }
</style>
