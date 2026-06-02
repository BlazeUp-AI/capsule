<script>
  import { onMount, onDestroy, tick } from 'svelte';

  let { sessionId, active } = $props();

  // Tree state
  let tree = $state({}); // path -> { name, type, children: [], expanded: bool, loaded: bool }
  let treeLoading = $state(false);

  // Tabs + editor state
  let openTabs = $state([]); // [{ path, name, content, modified }]
  let activeTabPath = $state(null);
  let editorEl = $state(null);
  let editor = null;
  let monacoModule = null;
  let models = {}; // path -> monaco model

  onMount(() => {
    if (sessionId) loadTree('/workspace');
  });

  onDestroy(() => {
    if (editor) { editor.dispose(); editor = null; }
    Object.values(models).forEach(m => m.dispose());
    models = {};
  });

  $effect(() => {
    if (active && sessionId && Object.keys(tree).length === 0) {
      loadTree('/workspace');
    }
  });

  $effect(() => {
    if (active && editor) {
      setTimeout(() => editor.layout(), 20);
    }
  });

  // ── Tree loading ──────────────────────────────────────────────────────────

  async function loadTree(path) {
    if (!sessionId) return;
    if (path === '/workspace' && Object.keys(tree).length === 0) treeLoading = true;
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files?path=${encodeURIComponent(path)}`);
      if (!resp.ok) return;
      const entries = await resp.json();

      const sorted = entries.sort((a, b) => {
        if (a.type === 'd' && b.type !== 'd') return -1;
        if (a.type !== 'd' && b.type === 'd') return 1;
        return a.name.localeCompare(b.name);
      });

      const newTree = { ...tree };
      newTree[path] = { loaded: true, expanded: true, children: sorted };

      // Remove old children entries if folder was reloaded
      for (const key of Object.keys(newTree)) {
        if (key !== path && key.startsWith(path + '/') && !sorted.some(e => e.path === key)) {
          // keep sub-entries that are still valid
        }
      }

      tree = newTree;
    } catch (e) {
      // silently fail
    }
    treeLoading = false;
  }

  function toggleFolder(path) {
    const entry = tree[path];
    if (entry && entry.loaded) {
      tree = { ...tree, [path]: { ...entry, expanded: !entry.expanded } };
    } else {
      loadTree(path);
    }
  }

  function getChildren(path) {
    const entry = tree[path];
    if (!entry || !entry.expanded) return [];
    return entry.children || [];
  }

  // ── File opening / tabs ───────────────────────────────────────────────────

  async function openFile(file) {
    if (file.type === 'd') {
      toggleFolder(file.path);
      return;
    }

    // Already open? Just switch tab
    const existing = openTabs.find(t => t.path === file.path);
    if (existing) {
      activeTabPath = file.path;
      switchModel(file.path);
      return;
    }

    // Fetch content
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(file.path)}`);
      if (!resp.ok) return;
      const content = await resp.text();

      openTabs = [...openTabs, { path: file.path, name: file.name, content, modified: false }];
      activeTabPath = file.path;

      await tick();
      await ensureEditor();
      createModel(file.path, file.name, content);
      switchModel(file.path);
    } catch (e) {
      // ignore
    }
  }

  function closeTab(path) {
    const idx = openTabs.findIndex(t => t.path === path);
    if (idx === -1) return;

    // Dispose model
    if (models[path]) { models[path].dispose(); delete models[path]; }

    openTabs = openTabs.filter(t => t.path !== path);

    if (activeTabPath === path) {
      if (openTabs.length > 0) {
        const newIdx = Math.min(idx, openTabs.length - 1);
        activeTabPath = openTabs[newIdx].path;
        switchModel(activeTabPath);
      } else {
        activeTabPath = null;
        if (editor) editor.setModel(null);
      }
    }
  }

  // ── Monaco ────────────────────────────────────────────────────────────────

  async function ensureEditor() {
    if (editor) return;
    if (!editorEl) return;

    if (!monacoModule) {
      self.MonacoEnvironment = {
        getWorker(_, label) {
          if (label === 'json') {
            return new Worker(new URL('monaco-editor/esm/vs/language/json/json.worker.js', import.meta.url), { type: 'module' });
          }
          if (label === 'typescript' || label === 'javascript') {
            return new Worker(new URL('monaco-editor/esm/vs/language/typescript/ts.worker.js', import.meta.url), { type: 'module' });
          }
          if (label === 'css' || label === 'scss' || label === 'less') {
            return new Worker(new URL('monaco-editor/esm/vs/language/css/css.worker.js', import.meta.url), { type: 'module' });
          }
          if (label === 'html' || label === 'handlebars' || label === 'razor') {
            return new Worker(new URL('monaco-editor/esm/vs/language/html/html.worker.js', import.meta.url), { type: 'module' });
          }
          return new Worker(new URL('monaco-editor/esm/vs/editor/editor.worker.js', import.meta.url), { type: 'module' });
        }
      };
      monacoModule = await import('monaco-editor');

      // Define capsule dark theme
      monacoModule.editor.defineTheme('capsule-dark', {
        base: 'vs-dark',
        inherit: true,
        rules: [],
        colors: {
          'editor.background': '#1a1a1a',
          'editor.foreground': '#e8e3d9',
          'editorCursor.foreground': '#d97706',
          'editor.selectionBackground': '#d9770630',
          'editor.lineHighlightBackground': '#ffffff08',
          'editorLineNumber.foreground': '#4a4a4a',
          'editorLineNumber.activeForeground': '#d97706',
        }
      });
    }

    const monaco = monacoModule;
    editor = monaco.editor.create(editorEl, {
      theme: 'capsule-dark',
      fontFamily: '"JetBrains Mono", monospace',
      fontSize: 13,
      lineHeight: 20,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      automaticLayout: true,
      padding: { top: 8, bottom: 8 },
      lineNumbers: 'on',
      renderLineHighlight: 'line',
      cursorBlinking: 'smooth',
      smoothScrolling: true,
      bracketPairColorization: { enabled: true },
      tabSize: 2,
      wordWrap: 'off',
      overviewRulerLanes: 0,
      hideCursorInOverviewRuler: true,
      scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8 },
    });

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => saveActiveFile());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyW, () => { if (activeTabPath) closeTab(activeTabPath); });
  }

  function createModel(path, filename, content) {
    if (models[path]) return;
    const monaco = monacoModule;
    const lang = getLang(filename);
    const uri = monaco.Uri.parse(`file://${path}`);
    const model = monaco.editor.createModel(content, lang, uri);

    model.onDidChangeContent(() => {
      const tab = openTabs.find(t => t.path === path);
      if (tab) {
        const isModified = model.getValue() !== tab.content;
        if (tab.modified !== isModified) {
          openTabs = openTabs.map(t => t.path === path ? { ...t, modified: isModified } : t);
        }
      }
    });

    models[path] = model;
  }

  function switchModel(path) {
    if (!editor || !models[path]) return;
    editor.setModel(models[path]);
    editor.focus();
  }

  function getLang(filename) {
    const ext = filename.split('.').pop()?.toLowerCase();
    const map = {
      js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript',
      ts: 'typescript', tsx: 'typescript',
      py: 'python', rs: 'rust', go: 'go',
      md: 'markdown', json: 'json', jsonc: 'json',
      html: 'html', htm: 'html', svelte: 'html', vue: 'html',
      css: 'css', scss: 'scss', less: 'less',
      yml: 'yaml', yaml: 'yaml', toml: 'ini',
      sh: 'shell', bash: 'shell', zsh: 'shell',
      sql: 'sql', graphql: 'graphql',
      dockerfile: 'dockerfile',
      xml: 'xml', svg: 'xml',
      c: 'c', cpp: 'cpp', h: 'c', hpp: 'cpp',
      java: 'java', kt: 'kotlin', rb: 'ruby', php: 'php',
      r: 'r', swift: 'swift', dart: 'dart',
    };
    if (filename.toLowerCase() === 'dockerfile') return 'dockerfile';
    if (filename.toLowerCase() === 'makefile') return 'makefile';
    return map[ext] || 'plaintext';
  }

  async function saveActiveFile() {
    if (!activeTabPath || !models[activeTabPath] || !sessionId) return;
    const content = models[activeTabPath].getValue();
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(activeTabPath)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'text/plain' },
        body: content,
      });
      if (resp.ok) {
        openTabs = openTabs.map(t => t.path === activeTabPath ? { ...t, content, modified: false } : t);
      }
    } catch (e) { /* ignore */ }
  }

  // ── Recursive tree render helpers ─────────────────────────────────────────

  function getDepth(path) {
    const rel = path.replace('/workspace/', '').replace('/workspace', '');
    if (!rel) return 0;
    return rel.split('/').length;
  }
</script>

<div class="editor-layout">
  <!-- Sidebar: File tree -->
  <div class="sidebar">
    <div class="sidebar-header">EXPLORER</div>
    <div class="tree-scroll">
      {#if treeLoading}
        <div class="tree-msg">Loading...</div>
      {:else if Object.keys(tree).length === 0}
        <div class="tree-msg">No workspace files</div>
      {:else}
        {@render renderTree('/workspace', 0)}
      {/if}
    </div>
  </div>

  <!-- Main editor area -->
  <div class="main">
    {#if openTabs.length > 0}
      <!-- Tab bar -->
      <div class="tab-bar">
        {#each openTabs as tab}
          <button
            class="tab"
            class:active={tab.path === activeTabPath}
            onclick={() => { activeTabPath = tab.path; switchModel(tab.path); }}
          >
            <span class="tab-name">{tab.name}</span>
            {#if tab.modified}<span class="tab-dot"></span>{/if}
            <button class="tab-close" onclick={(e) => { e.stopPropagation(); closeTab(tab.path); }}>×</button>
          </button>
        {/each}
      </div>
      <!-- Monaco -->
      <div class="editor-container" bind:this={editorEl}></div>
    {:else}
      <div class="empty-state">
        <div class="empty-title">No file open</div>
        <div class="empty-hint">Select a file from the explorer</div>
      </div>
    {/if}
  </div>
</div>

{#snippet renderTree(parentPath, depth)}
  {#each getChildren(parentPath) as entry}
    {#if entry.type === 'd'}
      <button
        class="tree-node"
        style="padding-left: {12 + depth * 14}px"
        onclick={() => openFile(entry)}
      >
        <span class="tree-chevron" class:expanded={tree[entry.path]?.expanded}>▸</span>
        <span class="tree-folder-name">{entry.name}</span>
      </button>
      {#if tree[entry.path]?.expanded}
        {@render renderTree(entry.path, depth + 1)}
      {/if}
    {:else}
      <button
        class="tree-node tree-file"
        class:active={entry.path === activeTabPath}
        style="padding-left: {12 + depth * 14 + 16}px"
        onclick={() => openFile(entry)}
      >
        <span class="tree-file-name">{entry.name}</span>
      </button>
    {/if}
  {/each}
{/snippet}

<style>
  .editor-layout {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }

  /* ── Sidebar ── */
  .sidebar {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--bg-panel);
  }

  .sidebar-header {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.12em;
    color: var(--text-lo);
    padding: 10px 12px 6px;
    border-bottom: 1px solid var(--border);
  }

  .tree-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 0;
  }

  .tree-msg {
    font-size: 10px;
    color: var(--text-lo);
    padding: 12px;
  }

  .tree-node {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    background: none;
    border: none;
    font-family: var(--font);
    font-size: 11px;
    color: var(--text);
    cursor: pointer;
    text-align: left;
    padding: 2px 12px;
    line-height: 22px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tree-node:hover { background: rgba(255,255,255,0.03); }
  .tree-node.active { background: var(--accent-lo); }

  .tree-chevron {
    font-size: 8px;
    color: var(--text-lo);
    width: 12px;
    text-align: center;
    flex-shrink: 0;
    transition: transform 0.1s;
    display: inline-block;
  }
  .tree-chevron.expanded { transform: rotate(90deg); }

  .tree-folder-name { color: var(--text); }
  .tree-file-name { color: var(--text); opacity: 0.85; }
  .tree-file.active .tree-file-name { color: var(--accent); opacity: 1; }

  /* ── Main area ── */
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .tab-bar {
    display: flex;
    align-items: center;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    height: 32px;
    overflow-x: auto;
    flex-shrink: 0;
  }

  .tab-bar::-webkit-scrollbar { height: 0; }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    border-right: 1px solid var(--border);
    font-family: var(--font);
    font-size: 11px;
    color: var(--text-lo);
    padding: 0 10px;
    height: 100%;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .tab:hover { background: rgba(255,255,255,0.02); }
  .tab.active { color: var(--text); background: var(--bg-inset); }

  .tab-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }

  .tab-close {
    background: none;
    border: none;
    color: var(--text-lo);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0 2px;
    border-radius: 2px;
    opacity: 0;
  }
  .tab:hover .tab-close { opacity: 0.6; }
  .tab-close:hover { opacity: 1; background: rgba(255,255,255,0.1); }

  .editor-container {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .empty-title { color: var(--text-lo); font-size: 12px; }
  .empty-hint { color: var(--text-lo); font-size: 10px; opacity: 0.5; }
</style>
