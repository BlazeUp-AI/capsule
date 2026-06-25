<script>
  import { onMount, onDestroy, tick } from 'svelte';

  let { sessionId, active } = $props();

  // ── State ─────────────────────────────────────────────────────────────────

  // File tree
  let tree = $state({});
  let treeLoading = $state(false);
  let gitStatus = $state({});  // path -> status ('M', '?', 'A', 'D')

  // Tabs + editor
  let openTabs = $state([]);
  let activeTabPath = $state(null);
  let editorEl = $state(null);
  let editor = null;
  let diffEditor = null;
  let monacoModule = null;
  let models = {};

  // Output panel
  let outputVisible = $state(false);
  let outputContent = $state('');
  let outputTab = $state('output');
  let isRunning = $state(false);
  let outputPanelHeight = $state(200);
  let isResizingOutput = $state(false);

  // UI state
  let inlineInput = $state(null);
  let contextMenu = $state(null);
  let tabContextMenu = $state(null);
  let sidebarWidth = $state(220);
  let isResizingSidebar = $state(false);
  let statusMessage = $state('');

  // Command palette
  let commandPaletteOpen = $state(false);
  let commandQuery = $state('');
  let commandInputEl = $state(null);
  let commandSelectedIdx = $state(0);

  // Quick open (Ctrl+P)
  let quickOpenVisible = $state(false);
  let quickOpenQuery = $state('');
  let quickOpenInputEl = $state(null);
  let quickOpenSelectedIdx = $state(0);
  let allFiles = $state([]);

  // Global search (Ctrl+Shift+F)
  let searchPanelVisible = $state(false);
  let searchQuery = $state('');
  let searchResults = $state([]);
  let searchLoading = $state(false);
  let searchCaseSensitive = $state(false);
  let searchRegex = $state(false);
  let searchReplace = $state('');
  let searchShowReplace = $state(false);

  // Diff view
  let diffMode = $state(false);
  let diffOriginalContent = $state('');

  // Preview modes
  let previewMode = $state(null); // 'markdown' | 'image' | null
  let markdownHtml = $state('');

  // Keyboard shortcuts overlay
  let shortcutsVisible = $state(false);

  // Persistent settings
  let settings = $state({
    fontSize: 13,
    wordWrap: 'off',
    minimap: true,
    tabSize: 2,
  });

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  onMount(() => {
    loadSettings();
    if (sessionId) {
      loadTree('/workspace');
      loadGitStatus();
      buildFileIndex();
    }
    document.addEventListener('click', closeAllMenus);
    document.addEventListener('keydown', handleGlobalKeydown);
    window.addEventListener('beforeunload', handleBeforeUnload);
  });

  onDestroy(() => {
    if (editor) { editor.dispose(); editor = null; }
    if (diffEditor) { diffEditor.dispose(); diffEditor = null; }
    Object.values(models).forEach(m => m.dispose());
    models = {};
    document.removeEventListener('click', closeAllMenus);
    document.removeEventListener('keydown', handleGlobalKeydown);
    window.removeEventListener('beforeunload', handleBeforeUnload);
  });

  $effect(() => {
    if (active && sessionId && Object.keys(tree).length === 0) {
      loadTree('/workspace');
      loadGitStatus();
      buildFileIndex();
    }
  });

  $effect(() => {
    if (active && editor) setTimeout(() => editor.layout(), 20);
  });

  // ── Persistent settings ───────────────────────────────────────────────────

  function loadSettings() {
    try {
      const saved = localStorage.getItem('capsule-editor-settings');
      if (saved) settings = { ...settings, ...JSON.parse(saved) };
    } catch (e) { /* ignore */ }
  }

  function saveSettings() {
    try { localStorage.setItem('capsule-editor-settings', JSON.stringify(settings)); } catch (e) { /* ignore */ }
  }

  function updateSetting(key, value) {
    settings = { ...settings, [key]: value };
    saveSettings();
    if (editor) {
      if (key === 'fontSize') editor.updateOptions({ fontSize: value });
      if (key === 'wordWrap') editor.updateOptions({ wordWrap: value });
      if (key === 'minimap') editor.updateOptions({ minimap: { enabled: value } });
      if (key === 'tabSize') editor.updateOptions({ tabSize: value });
    }
  }

  // ── Before unload warning ─────────────────────────────────────────────────

  function handleBeforeUnload(e) {
    if (openTabs.some(t => t.modified)) {
      e.preventDefault();
      e.returnValue = '';
    }
  }

  // ── Global keybindings ────────────────────────────────────────────────────

  function handleGlobalKeydown(e) {
    if (!active) return;

    // Ctrl+P — quick open
    if (e.ctrlKey && !e.shiftKey && e.key === 'p') {
      e.preventDefault();
      openQuickOpen();
      return;
    }
    // Ctrl+Shift+P — command palette
    if (e.ctrlKey && e.shiftKey && e.key === 'P') {
      e.preventDefault();
      toggleCommandPalette();
      return;
    }
    // Ctrl+Shift+F — global search
    if (e.ctrlKey && e.shiftKey && e.key === 'F') {
      e.preventDefault();
      toggleSearchPanel();
      return;
    }
    // Ctrl+` — toggle output
    if (e.ctrlKey && e.key === '`') {
      e.preventDefault();
      toggleOutput();
      return;
    }
    // Ctrl+B — toggle sidebar
    if (e.ctrlKey && e.key === 'b') {
      e.preventDefault();
      sidebarWidth = sidebarWidth > 0 ? 0 : 220;
      if (editor) setTimeout(() => editor.layout(), 20);
      return;
    }
    // Ctrl+Shift+? — shortcuts
    if (e.ctrlKey && e.shiftKey && e.key === '?') {
      e.preventDefault();
      shortcutsVisible = !shortcutsVisible;
      return;
    }
    // Ctrl+Tab — next tab
    if (e.ctrlKey && e.key === 'Tab') {
      e.preventDefault();
      cycleTab(e.shiftKey ? -1 : 1);
      return;
    }
    // Escape — close overlays
    if (e.key === 'Escape') {
      if (quickOpenVisible) { quickOpenVisible = false; return; }
      if (commandPaletteOpen) { commandPaletteOpen = false; return; }
      if (shortcutsVisible) { shortcutsVisible = false; return; }
      if (contextMenu) { closeAllMenus(); return; }
      if (inlineInput) { inlineInput = null; return; }
      if (diffMode) { exitDiffMode(); return; }
    }
  }

  function cycleTab(dir) {
    if (openTabs.length < 2) return;
    const idx = openTabs.findIndex(t => t.path === activeTabPath);
    const next = (idx + dir + openTabs.length) % openTabs.length;
    activeTabPath = openTabs[next].path;
    switchModel(activeTabPath);
  }

  // ── File tree ─────────────────────────────────────────────────────────────

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
      tree = { ...tree, [path]: { loaded: true, expanded: true, children: sorted } };
    } catch (e) { /* silent */ }
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

  // ── Git status ────────────────────────────────────────────────────────────

  async function loadGitStatus() {
    if (!sessionId) return;
    try {
      const result = await execInContainer(['git', '-C', '/workspace', 'status', '--porcelain']);
      if (!result || !result.stdout) return;
      const status = {};
      for (const line of result.stdout.split('\n')) {
        if (!line.trim()) continue;
        const code = line.substring(0, 2).trim();
        const file = line.substring(3).trim();
        status['/workspace/' + file] = code;
      }
      gitStatus = status;
    } catch (e) { /* ignore */ }
  }

  function getGitStatusClass(path) {
    const s = gitStatus[path];
    if (!s) return '';
    if (s === '??' || s === 'A') return 'git-new';
    if (s.includes('M')) return 'git-modified';
    if (s.includes('D')) return 'git-deleted';
    return '';
  }

  // ── Quick open (Ctrl+P) ──────────────────────────────────────────────────

  async function buildFileIndex() {
    if (!sessionId) return;
    try {
      const result = await execInContainer(['find', '/workspace', '-type', 'f', '-not', '-path', '*/node_modules/*', '-not', '-path', '*/.git/*', '-not', '-path', '*/target/*']);
      if (!result || !result.stdout) return;
      allFiles = result.stdout.split('\n').filter(f => f.trim()).map(f => ({
        path: f.trim(),
        name: f.trim().split('/').pop(),
        relativePath: f.trim().replace('/workspace/', ''),
      }));
    } catch (e) { /* ignore */ }
  }

  function openQuickOpen() {
    quickOpenVisible = true;
    quickOpenQuery = '';
    quickOpenSelectedIdx = 0;
    tick().then(() => { if (quickOpenInputEl) quickOpenInputEl.focus(); });
  }

  function getQuickOpenResults() {
    if (!quickOpenQuery) return allFiles.slice(0, 20);
    const q = quickOpenQuery.toLowerCase();
    const scored = allFiles.map(f => {
      const rel = f.relativePath.toLowerCase();
      const name = f.name.toLowerCase();
      let score = 0;
      if (name === q) score = 100;
      else if (name.startsWith(q)) score = 80;
      else if (name.includes(q)) score = 60;
      else if (rel.includes(q)) score = 40;
      else {
        // fuzzy match
        let qi = 0;
        for (let i = 0; i < rel.length && qi < q.length; i++) {
          if (rel[i] === q[qi]) qi++;
        }
        if (qi === q.length) score = 20;
      }
      return { ...f, score };
    }).filter(f => f.score > 0);
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 20);
  }

  function quickOpenSelect(file) {
    quickOpenVisible = false;
    openFile({ path: file.path, name: file.name, type: 'f' });
  }

  function handleQuickOpenKeydown(e) {
    const results = getQuickOpenResults();
    if (e.key === 'ArrowDown') { e.preventDefault(); quickOpenSelectedIdx = Math.min(quickOpenSelectedIdx + 1, results.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); quickOpenSelectedIdx = Math.max(quickOpenSelectedIdx - 1, 0); }
    else if (e.key === 'Enter') { e.preventDefault(); if (results[quickOpenSelectedIdx]) quickOpenSelect(results[quickOpenSelectedIdx]); }
    else if (e.key === 'Escape') { quickOpenVisible = false; }
  }

  // ── Global search (Ctrl+Shift+F) ─────────────────────────────────────────

  function toggleSearchPanel() {
    searchPanelVisible = !searchPanelVisible;
    if (searchPanelVisible) tick().then(() => { document.querySelector('.search-input')?.focus(); });
  }

  async function performSearch() {
    if (!searchQuery.trim() || !sessionId) return;
    searchLoading = true;
    searchResults = [];
    try {
      const flags = searchCaseSensitive ? '' : '-i';
      const pattern = searchRegex ? searchQuery : searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const cmd = `grep -rn ${flags} --include='*' --exclude-dir=node_modules --exclude-dir=.git --exclude-dir=target "${pattern}" /workspace 2>/dev/null | head -200`;
      const result = await execInContainer(['sh', '-c', cmd]);
      if (result && result.stdout) {
        const lines = result.stdout.split('\n').filter(l => l.trim());
        searchResults = lines.map(line => {
          const firstColon = line.indexOf(':');
          const secondColon = line.indexOf(':', firstColon + 1);
          if (firstColon === -1 || secondColon === -1) return null;
          return {
            path: line.substring(0, firstColon),
            line: parseInt(line.substring(firstColon + 1, secondColon)),
            text: line.substring(secondColon + 1).trim(),
            relativePath: line.substring(0, firstColon).replace('/workspace/', ''),
          };
        }).filter(Boolean);
      }
    } catch (e) { /* ignore */ }
    searchLoading = false;
  }

  async function searchReplaceAll() {
    if (!searchQuery.trim() || !searchReplace || !sessionId) return;
    const pattern = searchRegex ? searchQuery : searchQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const flags = searchCaseSensitive ? 'g' : 'gi';
    const cmd = `find /workspace -type f -not -path '*/node_modules/*' -not -path '*/.git/*' -not -path '*/target/*' -exec sed -i 's/${pattern}/${searchReplace}/${flags}' {} +`;
    await execInContainer(['sh', '-c', cmd]);
    flashStatus(`Replaced all occurrences`);
    await performSearch();
  }

  function openSearchResult(result) {
    openFile({ path: result.path, name: result.path.split('/').pop(), type: 'f' }).then(() => {
      if (editor) {
        editor.revealLineInCenter(result.line);
        editor.setPosition({ lineNumber: result.line, column: 1 });
        editor.focus();
      }
    });
  }

  // ── File opening / tabs ───────────────────────────────────────────────────

  async function openFile(file) {
    if (file.type === 'd') { toggleFolder(file.path); return; }

    // Check for preview modes
    const ext = file.name.split('.').pop()?.toLowerCase();
    const imageExts = ['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico', 'bmp'];

    if (imageExts.includes(ext)) {
      openTabs = openTabs.filter(t => t.path !== file.path);
      openTabs = [...openTabs, { path: file.path, name: file.name, content: '', modified: false, preview: 'image' }];
      activeTabPath = file.path;
      previewMode = 'image';
      return;
    }

    const existing = openTabs.find(t => t.path === file.path);
    if (existing) { activeTabPath = file.path; previewMode = null; switchModel(file.path); checkMarkdown(file.name); return; }

    try {
      const resp = await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(file.path)}`);
      if (!resp.ok) return;
      const content = await resp.text();
      openTabs = [...openTabs, { path: file.path, name: file.name, content, modified: false }];
      activeTabPath = file.path;
      previewMode = null;
      await tick();
      await ensureEditor();
      createModel(file.path, file.name, content);
      switchModel(file.path);
      checkMarkdown(file.name);
      if (editor) setTimeout(() => editor.layout(), 0);
    } catch (e) { /* ignore */ }
  }

  function checkMarkdown(filename) {
    const ext = filename.split('.').pop()?.toLowerCase();
    if (ext === 'md' || ext === 'markdown') {
      previewMode = 'markdown';
      updateMarkdownPreview();
    } else {
      previewMode = null;
    }
  }

  function updateMarkdownPreview() {
    if (!activeTabPath || !models[activeTabPath]) return;
    const content = models[activeTabPath].getValue();
    markdownHtml = renderMarkdown(content);
  }

  function renderMarkdown(text) {
    // Simple markdown renderer
    return text
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/^### (.+)$/gm, '<h3>$1</h3>')
      .replace(/^## (.+)$/gm, '<h2>$1</h2>')
      .replace(/^# (.+)$/gm, '<h1>$1</h1>')
      .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
      .replace(/\*(.+?)\*/g, '<em>$1</em>')
      .replace(/`([^`]+)`/g, '<code>$1</code>')
      .replace(/```[\s\S]*?```/g, (m) => `<pre>${m.slice(3, -3)}</pre>`)
      .replace(/^\- (.+)$/gm, '<li>$1</li>')
      .replace(/^(\d+)\. (.+)$/gm, '<li>$2</li>')
      .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
      .replace(/^---$/gm, '<hr/>')
      .replace(/\n\n/g, '<br/><br/>');
  }

  function closeTab(path) {
    const tab = openTabs.find(t => t.path === path);
    if (tab?.modified && !confirm(`"${tab.name}" has unsaved changes. Close anyway?`)) return;
    const idx = openTabs.findIndex(t => t.path === path);
    if (idx === -1) return;
    if (models[path]) { models[path].dispose(); delete models[path]; }
    openTabs = openTabs.filter(t => t.path !== path);
    if (activeTabPath === path) {
      if (openTabs.length > 0) {
        const newIdx = Math.min(idx, openTabs.length - 1);
        activeTabPath = openTabs[newIdx].path;
        previewMode = null;
        switchModel(activeTabPath);
      } else {
        activeTabPath = null;
        previewMode = null;
        if (editor) editor.setModel(null);
      }
    }
  }

  function closeOtherTabs(path) {
    const unsaved = openTabs.filter(t => t.path !== path && t.modified);
    if (unsaved.length > 0 && !confirm(`${unsaved.length} unsaved file(s) will be closed. Continue?`)) return;
    openTabs.filter(t => t.path !== path).forEach(t => {
      if (models[t.path]) { models[t.path].dispose(); delete models[t.path]; }
    });
    openTabs = openTabs.filter(t => t.path === path);
    activeTabPath = path;
    switchModel(path);
  }

  function closeTabsToRight(path) {
    const idx = openTabs.findIndex(t => t.path === path);
    const toClose = openTabs.slice(idx + 1);
    toClose.forEach(t => { if (models[t.path]) { models[t.path].dispose(); delete models[t.path]; } });
    openTabs = openTabs.slice(0, idx + 1);
    if (!openTabs.find(t => t.path === activeTabPath)) {
      activeTabPath = path;
      switchModel(path);
    }
  }

  function closeAllTabs() {
    const unsaved = openTabs.filter(t => t.modified);
    if (unsaved.length > 0 && !confirm(`${unsaved.length} unsaved file(s) will be closed. Continue?`)) return;
    openTabs.forEach(t => { if (models[t.path]) { models[t.path].dispose(); delete models[t.path]; } });
    openTabs = [];
    activeTabPath = null;
    previewMode = null;
    if (editor) editor.setModel(null);
  }

  // ── Monaco editor ─────────────────────────────────────────────────────────

  async function ensureEditor() {
    if (editor) return;
    if (!editorEl) return;

    if (!monacoModule) {
      self.MonacoEnvironment = {
        getWorker(_, label) {
          if (label === 'json') return new Worker(new URL('monaco-editor/esm/vs/language/json/json.worker.js', import.meta.url), { type: 'module' });
          if (label === 'typescript' || label === 'javascript') return new Worker(new URL('monaco-editor/esm/vs/language/typescript/ts.worker.js', import.meta.url), { type: 'module' });
          if (label === 'css' || label === 'scss' || label === 'less') return new Worker(new URL('monaco-editor/esm/vs/language/css/css.worker.js', import.meta.url), { type: 'module' });
          if (label === 'html' || label === 'handlebars' || label === 'razor') return new Worker(new URL('monaco-editor/esm/vs/language/html/html.worker.js', import.meta.url), { type: 'module' });
          return new Worker(new URL('monaco-editor/esm/vs/editor/editor.worker.js', import.meta.url), { type: 'module' });
        }
      };
      monacoModule = await import('monaco-editor');
      monacoModule.editor.defineTheme('capsule-dark', {
        base: 'vs-dark',
        inherit: true,
        rules: [
          { token: 'comment', foreground: '6b6560', fontStyle: 'italic' },
          { token: 'keyword', foreground: 'd97706' },
          { token: 'string', foreground: '7ee787' },
          { token: 'number', foreground: '79c0ff' },
          { token: 'type', foreground: 'd2a8ff' },
        ],
        colors: {
          'editor.background': '#1a1a1a',
          'editor.foreground': '#e8e3d9',
          'editorCursor.foreground': '#d97706',
          'editor.selectionBackground': '#d9770630',
          'editor.lineHighlightBackground': '#ffffff06',
          'editorLineNumber.foreground': '#3d3d3d',
          'editorLineNumber.activeForeground': '#d97706',
          'editorIndentGuide.background': '#2e2e2e',
          'editorIndentGuide.activeBackground': '#3d3d3d',
          'editorBracketMatch.background': '#d9770620',
          'editorBracketMatch.border': '#d97706',
          'editor.findMatchBackground': '#d9770640',
          'editor.findMatchHighlightBackground': '#d9770620',
          'editorWidget.background': '#242424',
          'editorWidget.border': '#2e2e2e',
          'input.background': '#181818',
          'input.border': '#3d3d3d',
          'input.foreground': '#e8e3d9',
          'list.activeSelectionBackground': '#d9770630',
          'list.hoverBackground': '#ffffff08',
        }
      });
    }

    editor = monacoModule.editor.create(editorEl, {
      theme: 'capsule-dark',
      fontFamily: '"JetBrains Mono", "Fira Code", monospace',
      fontSize: settings.fontSize,
      lineHeight: 20,
      fontLigatures: true,
      minimap: { enabled: settings.minimap, maxColumn: 80, renderCharacters: false },
      scrollBeyondLastLine: true,
      automaticLayout: true,
      padding: { top: 12, bottom: 12 },
      lineNumbers: 'on',
      renderLineHighlight: 'all',
      renderLineHighlightOnlyWhenFocus: false,
      cursorBlinking: 'smooth',
      cursorSmoothCaretAnimation: 'on',
      smoothScrolling: true,
      bracketPairColorization: { enabled: true },
      guides: { indentation: true, bracketPairs: true },
      tabSize: settings.tabSize,
      insertSpaces: true,
      wordWrap: settings.wordWrap,
      overviewRulerLanes: 2,
      scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
      suggest: { showMethods: true, showFunctions: true, showConstructors: true, showFields: true, showVariables: true, showClasses: true, showInterfaces: true, showModules: true, showProperties: true, showKeywords: true, showSnippets: true },
      quickSuggestions: { other: true, comments: false, strings: true },
      parameterHints: { enabled: true },
      formatOnPaste: true,
      formatOnType: true,
      autoClosingBrackets: 'always',
      autoClosingQuotes: 'always',
      autoIndent: 'full',
      linkedEditing: true,
      matchBrackets: 'always',
      folding: true,
      foldingStrategy: 'indentation',
      showFoldingControls: 'mouseover',
      find: { addExtraSpaceOnTop: false, autoFindInSelection: 'multiline', seedSearchStringFromSelection: 'selection' },
      contextmenu: true,
      mouseWheelZoom: true,
      dragAndDrop: true,
      links: true,
      colorDecorators: true,
      renderWhitespace: 'selection',
      renderControlCharacters: true,
      stickyScroll: { enabled: true },
    });

    const monaco = monacoModule;
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => saveActiveFile());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyW, () => { if (activeTabPath) closeTab(activeTabPath); });
    editor.addCommand(monaco.KeyCode.F5, () => runActiveFile());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Backquote, () => toggleOutput());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyP, () => toggleCommandPalette());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyP, () => openQuickOpen());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyF, () => toggleSearchPanel());
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyB, () => { sidebarWidth = sidebarWidth > 0 ? 0 : 220; setTimeout(() => editor.layout(), 20); });

    // Update markdown preview on content change
    editor.onDidChangeModelContent(() => {
      if (previewMode === 'markdown') updateMarkdownPreview();
    });
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
    if (diffMode) exitDiffMode();
    editor.setModel(models[path]);
    editor.focus();
  }

  function getLang(filename) {
    const ext = filename.split('.').pop()?.toLowerCase();
    const map = {
      js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript',
      ts: 'typescript', tsx: 'typescript', py: 'python', rs: 'rust', go: 'go',
      md: 'markdown', json: 'json', jsonc: 'json',
      html: 'html', htm: 'html', svelte: 'html', vue: 'html',
      css: 'css', scss: 'scss', less: 'less',
      yml: 'yaml', yaml: 'yaml', toml: 'ini',
      sh: 'shell', bash: 'shell', zsh: 'shell',
      sql: 'sql', graphql: 'graphql', dockerfile: 'dockerfile',
      xml: 'xml', svg: 'xml', c: 'c', cpp: 'cpp', h: 'c', hpp: 'cpp',
      java: 'java', kt: 'kotlin', rb: 'ruby', php: 'php',
      r: 'r', swift: 'swift', dart: 'dart',
    };
    if (filename.toLowerCase() === 'dockerfile') return 'dockerfile';
    if (filename.toLowerCase() === 'makefile') return 'makefile';
    return map[ext] || 'plaintext';
  }

  // ── Diff view ─────────────────────────────────────────────────────────────

  async function showDiff() {
    if (!activeTabPath || !sessionId) return;
    try {
      const result = await execInContainer(['git', '-C', '/workspace', 'show', `HEAD:${activeTabPath.replace('/workspace/', '')}`]);
      if (!result) { flashStatus('No git history'); return; }
      diffOriginalContent = result.stdout || '';
      diffMode = true;
      await tick();
      if (editor) editor.dispose();
      const diffEl = editorEl;
      if (!diffEl) return;
      const monaco = monacoModule;
      const original = monaco.editor.createModel(diffOriginalContent, getLang(getActiveFileName()));
      const modified = models[activeTabPath];
      diffEditor = monaco.editor.createDiffEditor(diffEl, {
        theme: 'capsule-dark',
        fontFamily: '"JetBrains Mono", "Fira Code", monospace',
        fontSize: settings.fontSize,
        automaticLayout: true,
        renderSideBySide: true,
        readOnly: false,
      });
      diffEditor.setModel({ original, modified });
    } catch (e) { flashStatus('Diff failed'); }
  }

  function exitDiffMode() {
    diffMode = false;
    if (diffEditor) { diffEditor.dispose(); diffEditor = null; }
    editor = null;
    tick().then(() => {
      ensureEditor().then(() => {
        if (activeTabPath && models[activeTabPath]) switchModel(activeTabPath);
      });
    });
  }

  // ── Save ──────────────────────────────────────────────────────────────────

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
        flashStatus('Saved');
        loadGitStatus();
      }
    } catch (e) { flashStatus('Save failed'); }
  }

  async function saveAllFiles() {
    for (const tab of openTabs.filter(t => t.modified)) {
      if (!models[tab.path]) continue;
      const content = models[tab.path].getValue();
      try {
        const resp = await fetch(`/api/sessions/${sessionId}/files/content?path=${encodeURIComponent(tab.path)}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'text/plain' },
          body: content,
        });
        if (resp.ok) {
          openTabs = openTabs.map(t => t.path === tab.path ? { ...t, content, modified: false } : t);
        }
      } catch (e) { /* ignore */ }
    }
    flashStatus('All files saved');
    loadGitStatus();
  }

  // ── Run ───────────────────────────────────────────────────────────────────

  function getRunCommand(filepath) {
    const ext = filepath.split('.').pop()?.toLowerCase();
    const runners = {
      py: ['python3', filepath],
      js: ['node', filepath],
      mjs: ['node', filepath],
      ts: ['npx', 'tsx', filepath],
      sh: ['bash', filepath],
      bash: ['bash', filepath],
      rb: ['ruby', filepath],
      go: ['go', 'run', filepath],
      c: ['sh', '-c', `gcc ${filepath} -o /tmp/out && /tmp/out`],
      cpp: ['sh', '-c', `g++ ${filepath} -o /tmp/out && /tmp/out`],
      php: ['php', filepath],
      java: ['sh', '-c', `cd /workspace && javac ${filepath} && java ${filepath.replace('.java','').split('/').pop()}`],
    };
    return runners[ext] || ['sh', '-c', `chmod +x ${filepath} && ${filepath}`];
  }

  async function runActiveFile() {
    if (!activeTabPath || !sessionId || isRunning) return;
    await saveActiveFile();
    const command = getRunCommand(activeTabPath);
    isRunning = true;
    outputVisible = true;
    outputTab = 'output';
    const timestamp = new Date().toLocaleTimeString();
    outputContent = `[${timestamp}] $ ${command.join(' ')}\n\n`;

    try {
      const resp = await fetch(`/api/sessions/${sessionId}/exec`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ command, timeout: 30 }),
      });
      if (!resp.ok) { outputContent += `[Error: HTTP ${resp.status}]\n`; isRunning = false; return; }
      const result = await resp.json();
      if (result.stdout) outputContent += result.stdout;
      if (result.stderr) outputContent += (result.stdout && !result.stdout.endsWith('\n') ? '\n' : '') + result.stderr;
      outputContent += `\n\n[Exit code: ${result.exit_code}] ${result.exit_code === 0 ? '✓' : '✗'}`;
    } catch (e) {
      outputContent += `[Error: ${e.message}]\n`;
    }
    isRunning = false;
    if (editor) setTimeout(() => editor.layout(), 20);
  }

  async function runCustomCommand(cmd) {
    if (!sessionId || isRunning) return;
    isRunning = true;
    outputVisible = true;
    outputTab = 'output';
    const timestamp = new Date().toLocaleTimeString();
    outputContent = `[${timestamp}] $ ${cmd}\n\n`;
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/exec`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ command: ['sh', '-c', cmd], timeout: 60 }),
      });
      if (!resp.ok) { outputContent += `[Error: HTTP ${resp.status}]\n`; isRunning = false; return; }
      const result = await resp.json();
      if (result.stdout) outputContent += result.stdout;
      if (result.stderr) outputContent += (result.stdout && !result.stdout.endsWith('\n') ? '\n' : '') + result.stderr;
      outputContent += `\n\n[Exit code: ${result.exit_code}] ${result.exit_code === 0 ? '✓' : '✗'}`;
    } catch (e) { outputContent += `[Error: ${e.message}]\n`; }
    isRunning = false;
    if (editor) setTimeout(() => editor.layout(), 20);
  }

  // ── File operations ───────────────────────────────────────────────────────

  function startCreate(type, parentPath = null) {
    const target = parentPath || findActiveFolder();
    if (!tree[target]?.expanded) toggleFolder(target);
    inlineInput = { parentPath: target, type, value: '' };
    tick().then(() => { const el = document.querySelector('.inline-input input'); if (el) el.focus(); });
  }

  function findActiveFolder() {
    if (activeTabPath) { const parts = activeTabPath.split('/'); parts.pop(); return parts.join('/'); }
    return '/workspace';
  }

  async function commitCreate() {
    if (!inlineInput || !inlineInput.value.trim()) { inlineInput = null; return; }
    const name = inlineInput.value.trim();
    const parentPath = inlineInput.parentPath;
    const fullPath = `${parentPath}/${name}`;
    const isFolder = inlineInput.type === 'folder';
    if (isFolder) { await execInContainer(['mkdir', '-p', fullPath]); }
    else { await execInContainer(['touch', fullPath]); }
    inlineInput = null;
    await loadTree(parentPath);
    await buildFileIndex();
    loadGitStatus();
    if (!isFolder) openFile({ path: fullPath, name, type: 'f' });
  }

  function cancelCreate() { inlineInput = null; }

  function showContextMenu(e, entry) { e.preventDefault(); e.stopPropagation(); contextMenu = { x: e.clientX, y: e.clientY, entry }; }
  function showTabContextMenu(e, tab) { e.preventDefault(); e.stopPropagation(); tabContextMenu = { x: e.clientX, y: e.clientY, tab }; }
  function closeAllMenus() { contextMenu = null; tabContextMenu = null; }

  async function deleteEntry(entry) {
    closeAllMenus();
    if (!confirm(`Delete "${entry.name}"?`)) return;
    await execInContainer(['rm', '-rf', entry.path]);
    if (entry.type !== 'd') closeTab(entry.path);
    await loadTree(entry.path.split('/').slice(0, -1).join('/'));
    await buildFileIndex();
    loadGitStatus();
  }

  function startRename(entry) {
    closeAllMenus();
    inlineInput = { path: entry.path, type: 'rename', value: entry.name, entry };
    tick().then(() => { const el = document.querySelector('.inline-input input'); if (el) { el.focus(); el.select(); } });
  }

  async function commitRename() {
    if (!inlineInput || inlineInput.type !== 'rename') { inlineInput = null; return; }
    const newName = inlineInput.value.trim();
    if (!newName || newName === inlineInput.entry.name) { inlineInput = null; return; }
    const oldPath = inlineInput.path;
    const parentPath = oldPath.split('/').slice(0, -1).join('/');
    const newPath = `${parentPath}/${newName}`;
    await execInContainer(['mv', oldPath, newPath]);
    if (models[oldPath]) { models[oldPath].dispose(); delete models[oldPath]; }
    openTabs = openTabs.filter(t => t.path !== oldPath);
    if (activeTabPath === oldPath) activeTabPath = null;
    inlineInput = null;
    await loadTree(parentPath);
    await buildFileIndex();
    loadGitStatus();
  }

  async function duplicateEntry(entry) {
    closeAllMenus();
    const ext = entry.name.includes('.') ? '.' + entry.name.split('.').pop() : '';
    const base = entry.name.replace(ext, '');
    const newPath = `${entry.path.split('/').slice(0, -1).join('/')}/${base}-copy${ext}`;
    await execInContainer(['cp', '-r', entry.path, newPath]);
    await loadTree(entry.path.split('/').slice(0, -1).join('/'));
    await buildFileIndex();
  }

  // ── Command palette ───────────────────────────────────────────────────────

  function toggleCommandPalette() {
    commandPaletteOpen = !commandPaletteOpen;
    commandQuery = '';
    commandSelectedIdx = 0;
    if (commandPaletteOpen) tick().then(() => { if (commandInputEl) commandInputEl.focus(); });
  }

  function getCommands() {
    const all = [
      { label: 'Quick Open File', shortcut: 'Ctrl+P', action: openQuickOpen },
      { label: 'Global Search', shortcut: 'Ctrl+Shift+F', action: toggleSearchPanel },
      { label: 'Run File', shortcut: 'F5', action: runActiveFile },
      { label: 'Save File', shortcut: 'Ctrl+S', action: saveActiveFile },
      { label: 'Save All', shortcut: '', action: saveAllFiles },
      { label: 'Close Tab', shortcut: 'Ctrl+W', action: () => { if (activeTabPath) closeTab(activeTabPath); } },
      { label: 'Close All Tabs', shortcut: '', action: closeAllTabs },
      { label: 'Toggle Output', shortcut: 'Ctrl+`', action: toggleOutput },
      { label: 'Toggle Sidebar', shortcut: 'Ctrl+B', action: () => { sidebarWidth = sidebarWidth > 0 ? 0 : 220; } },
      { label: 'New File', shortcut: '', action: () => startCreate('file') },
      { label: 'New Folder', shortcut: '', action: () => startCreate('folder') },
      { label: 'Refresh Explorer', shortcut: '', action: () => { loadTree('/workspace'); buildFileIndex(); loadGitStatus(); } },
      { label: 'Show Git Diff', shortcut: '', action: showDiff },
      { label: 'Toggle Markdown Preview', shortcut: '', action: () => { previewMode = previewMode === 'markdown' ? null : 'markdown'; if (previewMode === 'markdown') updateMarkdownPreview(); } },
      { label: 'Keyboard Shortcuts', shortcut: 'Ctrl+Shift+?', action: () => { shortcutsVisible = true; } },
      { label: 'Format Document', shortcut: 'Shift+Alt+F', action: () => { if (editor) editor.getAction('editor.action.formatDocument')?.run(); } },
      { label: 'Find in File', shortcut: 'Ctrl+F', action: () => { if (editor) editor.getAction('actions.find')?.run(); } },
      { label: 'Replace in File', shortcut: 'Ctrl+H', action: () => { if (editor) editor.getAction('editor.action.startFindReplaceAction')?.run(); } },
      { label: 'Go to Line', shortcut: 'Ctrl+G', action: () => { if (editor) editor.getAction('editor.action.gotoLine')?.run(); } },
      { label: 'Toggle Word Wrap', shortcut: '', action: () => { const v = settings.wordWrap === 'off' ? 'on' : 'off'; updateSetting('wordWrap', v); } },
      { label: 'Toggle Minimap', shortcut: '', action: () => updateSetting('minimap', !settings.minimap) },
      { label: 'Increase Font Size', shortcut: '', action: () => updateSetting('fontSize', settings.fontSize + 1) },
      { label: 'Decrease Font Size', shortcut: '', action: () => updateSetting('fontSize', Math.max(8, settings.fontSize - 1)) },
    ];
    if (!commandQuery) return all;
    const q = commandQuery.toLowerCase();
    return all.filter(c => c.label.toLowerCase().includes(q));
  }

  function executeCommand(cmd) {
    commandPaletteOpen = false;
    commandQuery = '';
    cmd.action();
  }

  function handleCommandKeydown(e) {
    const cmds = getCommands();
    if (e.key === 'ArrowDown') { e.preventDefault(); commandSelectedIdx = Math.min(commandSelectedIdx + 1, cmds.length - 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); commandSelectedIdx = Math.max(commandSelectedIdx - 1, 0); }
    else if (e.key === 'Enter') { e.preventDefault(); if (cmds[commandSelectedIdx]) executeCommand(cmds[commandSelectedIdx]); }
    else if (e.key === 'Escape') { commandPaletteOpen = false; }
  }

  // ── Sidebar resize ────────────────────────────────────────────────────────

  function startSidebarResize(e) {
    isResizingSidebar = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    function onMove(e) { sidebarWidth = Math.max(120, Math.min(500, startWidth + (e.clientX - startX))); }
    function onUp() { isResizingSidebar = false; document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp); if (editor) editor.layout(); }
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }

  // ── Output panel resize ───────────────────────────────────────────────────

  function startOutputResize(e) {
    isResizingOutput = true;
    const startY = e.clientY;
    const startH = outputPanelHeight;
    function onMove(e) { outputPanelHeight = Math.max(80, Math.min(600, startH - (e.clientY - startY))); }
    function onUp() { isResizingOutput = false; document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp); if (editor) editor.layout(); }
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
  }

  // ── Helpers ───────────────────────────────────────────────────────────────

  async function execInContainer(command) {
    if (!sessionId) return null;
    try {
      const resp = await fetch(`/api/sessions/${sessionId}/exec`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ command, timeout: 10 }),
      });
      return resp.ok ? await resp.json() : null;
    } catch (e) { return null; }
  }

  function toggleOutput() {
    outputVisible = !outputVisible;
    if (editor) setTimeout(() => editor.layout(), 20);
  }

  function clearOutput() { outputContent = ''; }

  function flashStatus(msg) {
    statusMessage = msg;
    setTimeout(() => { if (statusMessage === msg) statusMessage = ''; }, 2000);
  }

  function getFileIcon(name) {
    const ext = name.split('.').pop()?.toLowerCase();
    const icons = { py: '🐍', js: '⬡', ts: '⬡', jsx: '⬡', tsx: '⬡', html: '◇', css: '◇', json: '{}', md: '¶', rs: '⚙', go: '◈', sh: '$', yml: '⚙', yaml: '⚙', toml: '⚙', sql: '⊞', svg: '◇', java: '☕', rb: '♦', php: '⬡', c: 'C', cpp: 'C', h: 'H', png: '🖼', jpg: '🖼', jpeg: '🖼', gif: '🖼', webp: '🖼' };
    return icons[ext] || '○';
  }

  function getActiveFileName() {
    if (!activeTabPath) return '';
    return activeTabPath.split('/').pop();
  }

  function getLangDisplay() {
    if (!activeTabPath) return '';
    return getLang(getActiveFileName()).charAt(0).toUpperCase() + getLang(getActiveFileName()).slice(1);
  }

  function getCursorPosition() {
    if (!editor) return '';
    const pos = editor.getPosition();
    if (!pos) return '';
    return `Ln ${pos.lineNumber}, Col ${pos.column}`;
  }

  function getBreadcrumbs() {
    if (!activeTabPath) return [];
    return activeTabPath.replace('/workspace/', '').split('/');
  }
</script>

<!-- ═══════════════════════════════════════════════════════════════════════════ -->
<!-- TEMPLATE                                                                   -->
<!-- ═══════════════════════════════════════════════════════════════════════════ -->

<div class="ide-layout">
  <!-- ── Sidebar ── -->
  {#if sidebarWidth > 0}
  <div class="sidebar" style="width: {sidebarWidth}px">
    <div class="sidebar-header">
      <span>EXPLORER</span>
      <div class="sidebar-actions">
        <button class="icon-btn" title="New File" onclick={() => startCreate('file')}>
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M9.5 1.1l3.4 3.4.1.5v10c0 .6-.4 1-1 1H4c-.6 0-1-.4-1-1V2c0-.6.4-1 1-1h5l.5.1zM9 5h3l-3-3v3zM4 15h8V6H8.5l-.5-.5V2H4v13z"/><path d="M7 7H8v3h3v1H8v3H7v-3H4v-1h3V7z"/></svg>
        </button>
        <button class="icon-btn" title="New Folder" onclick={() => startCreate('folder')}>
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M14 4H9.618l-1-2H2a1 1 0 00-1 1v10a1 1 0 001 1h12a1 1 0 001-1V5a1 1 0 00-1-1zm0 9H2V3h6.382l1 2H14v8z"/><path d="M7 7H8v2h2v1H8v2H7v-2H5V9h2V7z"/></svg>
        </button>
        <button class="icon-btn" title="Search (Ctrl+Shift+F)" onclick={toggleSearchPanel}>
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M11.742 10.344a6.5 6.5 0 10-1.397 1.398h-.001c.03.04.062.078.098.115l3.85 3.85a1 1 0 001.415-1.414l-3.85-3.85a1.007 1.007 0 00-.115-.1zM12 6.5a5.5 5.5 0 11-11 0 5.5 5.5 0 0111 0z"/></svg>
        </button>
        <button class="icon-btn" title="Refresh" onclick={() => { loadTree('/workspace'); buildFileIndex(); loadGitStatus(); }}>
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M13.451 5.609l-.579-.939-1.068.812-.076.094c-.335.415-.927 1.146-1.26 1.565l1.078.822c.2-.248.453-.563.684-.846A5.001 5.001 0 018 13a5 5 0 01-5-5 5 5 0 015-5c1.22 0 2.36.444 3.244 1.194l-1.494.012.012 1.5 3.24-.026.012-3.238-1.5-.012-.02 1.564A6.466 6.466 0 008 2a6 6 0 100 12 6.001 6.001 0 005.451-8.391z"/></svg>
        </button>
      </div>
    </div>

    <!-- Search panel in sidebar -->
    {#if searchPanelVisible}
    <div class="search-panel">
      <div class="search-row">
        <input class="search-input" type="text" bind:value={searchQuery} placeholder="Search in files..." onkeydown={(e) => { if (e.key === 'Enter') performSearch(); }} />
        <button class="icon-btn" class:active={searchCaseSensitive} title="Match Case" onclick={() => { searchCaseSensitive = !searchCaseSensitive; }}>Aa</button>
        <button class="icon-btn" class:active={searchRegex} title="Use Regex" onclick={() => { searchRegex = !searchRegex; }}>.*</button>
      </div>
      {#if searchShowReplace}
      <div class="search-row">
        <input class="search-input" type="text" bind:value={searchReplace} placeholder="Replace..." />
        <button class="icon-btn" title="Replace All" onclick={searchReplaceAll}>⟳</button>
      </div>
      {/if}
      <div class="search-actions">
        <button class="icon-btn" onclick={() => { searchShowReplace = !searchShowReplace; }}>↔</button>
        <button class="search-go" onclick={performSearch}>{searchLoading ? '...' : 'Search'}</button>
      </div>
      {#if searchResults.length > 0}
      <div class="search-results">
        <div class="search-count">{searchResults.length} result{searchResults.length !== 1 ? 's' : ''}</div>
        {#each searchResults as result}
          <button class="search-result" onclick={() => openSearchResult(result)}>
            <span class="search-result-file">{result.relativePath}:{result.line}</span>
            <span class="search-result-text">{result.text.substring(0, 80)}</span>
          </button>
        {/each}
      </div>
      {/if}
    </div>
    {:else}
    <div class="tree-scroll">
      {#if treeLoading}
        <div class="tree-msg">Loading...</div>
      {:else}
        {#if inlineInput && inlineInput.type !== 'rename' && inlineInput.parentPath === '/workspace'}
          <div class="inline-input" style="padding-left: 12px">
            <span class="inline-icon">{inlineInput.type === 'folder' ? '📁' : '📄'}</span>
            <input type="text" bind:value={inlineInput.value} placeholder={inlineInput.type === 'folder' ? 'folder name' : 'file name'} onkeydown={(e) => { if (e.key === 'Enter') commitCreate(); if (e.key === 'Escape') cancelCreate(); }} onblur={() => commitCreate()} />
          </div>
        {/if}
        {#if Object.keys(tree).length === 0 && !inlineInput}
          <div class="tree-msg">Empty workspace</div>
        {:else}
          {@render renderTree('/workspace', 0)}
        {/if}
      {/if}
    </div>
    {/if}
  </div>
  <!-- Sidebar resize handle -->
  <div class="resize-handle-v" role="separator" aria-label="Resize sidebar" tabindex="0" onmousedown={startSidebarResize}></div>
  {/if}

  <!-- ── Main ── -->
  <div class="main">
    <!-- Tab bar -->
    <div class="tab-bar">
      <div class="tab-bar-tabs">
        {#each openTabs as tab}
          <div class="tab" class:active={tab.path === activeTabPath} role="tab" tabindex="0" onclick={() => { activeTabPath = tab.path; previewMode = tab.preview || null; if (!tab.preview) switchModel(tab.path); }} onkeydown={(e) => { if (e.key === 'Enter') { activeTabPath = tab.path; switchModel(tab.path); } }} oncontextmenu={(e) => showTabContextMenu(e, tab)}>
            <span class="tab-icon">{getFileIcon(tab.name)}</span>
            <span class="tab-name">{tab.name}</span>
            {#if tab.modified}<span class="tab-dot"></span>{/if}
            <button class="tab-close" onclick={(e) => { e.stopPropagation(); closeTab(tab.path); }}>×</button>
          </div>
        {/each}
      </div>
      <div class="tab-bar-actions">
        {#if activeTabPath && !previewMode}
          <button class="run-btn" onclick={runActiveFile} disabled={isRunning} title="Run (F5)">
            {#if isRunning}
              <svg width="11" height="11" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="1"/></svg>
            {:else}
              <svg width="11" height="11" viewBox="0 0 16 16" fill="currentColor"><path d="M4 2l10 6-10 6V2z"/></svg>
            {/if}
            <span>Run</span>
          </button>
        {/if}
      </div>
    </div>

    <!-- Breadcrumb -->
    {#if activeTabPath}
    <div class="breadcrumb">
      {#each getBreadcrumbs() as crumb, i}
        {#if i > 0}<span class="breadcrumb-sep">›</span>{/if}
        <span class="breadcrumb-item">{crumb}</span>
      {/each}
      {#if diffMode}<span class="breadcrumb-sep">›</span><span class="breadcrumb-item breadcrumb-diff">DIFF</span>{/if}
    </div>
    {/if}

    <!-- Editor area -->
    <div class="editor-area">
      {#if previewMode === 'image'}
        <div class="preview-container">
          <img src="/api/sessions/{sessionId}/files/content?path={encodeURIComponent(activeTabPath)}" alt={getActiveFileName()} class="image-preview" />
        </div>
      {:else if previewMode === 'markdown'}
        <div class="editor-with-preview">
          <div class="editor-container" class:hidden={openTabs.length === 0} bind:this={editorEl}></div>
          <div class="markdown-preview">
            <div class="markdown-body">{@html markdownHtml}</div>
          </div>
        </div>
      {:else}
        <div class="editor-container" class:hidden={openTabs.length === 0} bind:this={editorEl}></div>
      {/if}
      {#if openTabs.length === 0 && !previewMode}
        <div class="empty-state">
          <div class="empty-icon">
            <svg width="48" height="48" viewBox="0 0 16 16" fill="currentColor" opacity="0.15"><path d="M9.5 1.1l3.4 3.4.1.5v10c0 .6-.4 1-1 1H4c-.6 0-1-.4-1-1V2c0-.6.4-1 1-1h5l.5.1zM9 5h3l-3-3v3zM4 15h8V6H8.5l-.5-.5V2H4v13z"/></svg>
          </div>
          <div class="empty-title">No file open</div>
          <div class="empty-hint">Open a file from the explorer, Ctrl+P to quick open, or Ctrl+Shift+P for commands</div>
          <div class="empty-shortcuts">
            <span>Ctrl+P — Quick Open</span>
            <span>Ctrl+Shift+P — Commands</span>
            <span>Ctrl+Shift+F — Search in Files</span>
            <span>F5 — Run File</span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Output panel -->
    {#if outputVisible}
      <div class="resize-handle-h" role="separator" aria-label="Resize output panel" tabindex="0" onmousedown={startOutputResize}></div>
      <div class="output-panel" style="height: {outputPanelHeight}px">
        <div class="output-header">
          <div class="output-tabs">
            <button class="output-tab" class:active={outputTab === 'output'} onclick={() => { outputTab = 'output'; }}>OUTPUT</button>
            <button class="output-tab" class:active={outputTab === 'problems'} onclick={() => { outputTab = 'problems'; }}>PROBLEMS</button>
          </div>
          <div class="output-actions">
            <button class="icon-btn" title="Clear" onclick={clearOutput}><svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M10 3h3v1h-1v9a1 1 0 01-1 1H5a1 1 0 01-1-1V4H3V3h3V2a1 1 0 011-1h2a1 1 0 011 1v1zM5 4v9h6V4H5zm1 2h1v5H6V6zm3 0h1v5H9V6z"/></svg></button>
            <button class="icon-btn" title="Close panel" onclick={toggleOutput}><svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M8 8.707l3.646 3.647.708-.707L8.707 8l3.647-3.646-.707-.708L8 7.293 4.354 3.646l-.707.708L7.293 8l-3.646 3.646.707.708L8 8.707z"/></svg></button>
          </div>
        </div>
        <div class="output-body">
          <pre class="output-content">{outputContent || 'Run a file (F5) to see output here.'}</pre>
        </div>
      </div>
    {/if}

    <!-- Status bar -->
    <div class="status-bar">
      <div class="status-left">
        <button class="status-toggle" onclick={toggleOutput} title="Toggle Output (Ctrl+`)">
          {#if outputVisible}▾ Output{:else}▸ Output{/if}
        </button>
        {#if statusMessage}
          <span class="status-flash">{statusMessage}</span>
        {:else if activeTabPath}
          <span class="status-path">{activeTabPath.replace('/workspace/', '')}</span>
        {/if}
      </div>
      <div class="status-right">
        {#if activeTabPath && !previewMode}
          <span class="status-item">{getCursorPosition()}</span>
          <span class="status-item">{getLangDisplay()}</span>
          <span class="status-item">UTF-8</span>
          <span class="status-item">Spaces: {settings.tabSize}</span>
        {/if}
        <button class="status-item status-btn" onclick={() => { shortcutsVisible = true; }}>⌨</button>
      </div>
    </div>
  </div>
</div>

<!-- ── Tree snippet ── -->
{#snippet renderTree(parentPath, depth)}
  {#each getChildren(parentPath) as entry}
    {#if inlineInput?.type === 'rename' && inlineInput.path === entry.path}
      <div class="inline-input" style="padding-left: {12 + depth * 14}px">
        <input type="text" bind:value={inlineInput.value} onkeydown={(e) => { if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') cancelCreate(); }} onblur={() => commitRename()} />
      </div>
    {:else if entry.type === 'd'}
      <button class="tree-node {getGitStatusClass(entry.path)}" style="padding-left: {12 + depth * 14}px" onclick={() => openFile(entry)} oncontextmenu={(e) => showContextMenu(e, entry)}>
        <span class="tree-chevron" class:expanded={tree[entry.path]?.expanded}>▸</span>
        <span class="tree-folder-icon">📁</span>
        <span class="tree-name">{entry.name}</span>
      </button>
      {#if tree[entry.path]?.expanded}
        {#if inlineInput && inlineInput.type !== 'rename' && inlineInput.parentPath === entry.path}
          <div class="inline-input" style="padding-left: {12 + (depth+1) * 14}px">
            <span class="inline-icon">{inlineInput.type === 'folder' ? '📁' : '📄'}</span>
            <input type="text" bind:value={inlineInput.value} placeholder={inlineInput.type === 'folder' ? 'folder name' : 'file name'} onkeydown={(e) => { if (e.key === 'Enter') commitCreate(); if (e.key === 'Escape') cancelCreate(); }} onblur={() => commitCreate()} />
          </div>
        {/if}
        {@render renderTree(entry.path, depth + 1)}
      {/if}
    {:else}
      <button class="tree-node tree-file {getGitStatusClass(entry.path)}" class:active={entry.path === activeTabPath} style="padding-left: {12 + depth * 14 + 16}px" onclick={() => openFile(entry)} oncontextmenu={(e) => showContextMenu(e, entry)}>
        <span class="tree-file-icon">{getFileIcon(entry.name)}</span>
        <span class="tree-name">{entry.name}</span>
      </button>
    {/if}
  {/each}
{/snippet}

<!-- ── Context menu ── -->
{#if contextMenu}
  <div class="context-menu" style="left: {contextMenu.x}px; top: {contextMenu.y}px">
    {#if contextMenu.entry.type === 'd'}
      <button onclick={() => { const e = contextMenu.entry; closeAllMenus(); startCreate('file', e.path); }}>New File</button>
      <button onclick={() => { const e = contextMenu.entry; closeAllMenus(); startCreate('folder', e.path); }}>New Folder</button>
      <div class="context-sep"></div>
    {/if}
    <button onclick={() => startRename(contextMenu.entry)}>Rename</button>
    <button onclick={() => duplicateEntry(contextMenu.entry)}>Duplicate</button>
    <div class="context-sep"></div>
    <button class="context-danger" onclick={() => deleteEntry(contextMenu.entry)}>Delete</button>
  </div>
{/if}

<!-- ── Tab context menu ── -->
{#if tabContextMenu}
  <div class="context-menu" style="left: {tabContextMenu.x}px; top: {tabContextMenu.y}px">
    <button onclick={() => { const p = tabContextMenu.tab.path; closeAllMenus(); closeTab(p); }}>Close</button>
    <button onclick={() => { const p = tabContextMenu.tab.path; closeAllMenus(); closeOtherTabs(p); }}>Close Others</button>
    <button onclick={() => { const p = tabContextMenu.tab.path; closeAllMenus(); closeTabsToRight(p); }}>Close to the Right</button>
    <button onclick={() => { closeAllMenus(); closeAllTabs(); }}>Close All</button>
    <div class="context-sep"></div>
    <button onclick={() => { const p = tabContextMenu.tab.path; closeAllMenus(); navigator.clipboard?.writeText(p.replace('/workspace/', '')); flashStatus('Path copied'); }}>Copy Path</button>
    {#if tabContextMenu.tab.modified}
      <button onclick={() => { activeTabPath = tabContextMenu.tab.path; closeAllMenus(); saveActiveFile(); }}>Save</button>
    {/if}
  </div>
{/if}

<!-- ── Quick Open (Ctrl+P) ── -->
{#if quickOpenVisible}
  <div class="palette-overlay" role="dialog" aria-modal="true" aria-label="Quick Open" onclick={() => { quickOpenVisible = false; }} onkeydown={(e) => { if (e.key === 'Escape') quickOpenVisible = false; }}>
    <div class="palette" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <input class="palette-input" type="text" bind:this={quickOpenInputEl} bind:value={quickOpenQuery} placeholder="Search files by name..." onkeydown={handleQuickOpenKeydown} />
      <div class="palette-list">
        {#each getQuickOpenResults() as file, i}
          <button class="palette-item" class:selected={i === quickOpenSelectedIdx} onclick={() => quickOpenSelect(file)}>
            <span class="palette-icon">{getFileIcon(file.name)}</span>
            <span class="palette-label">{file.name}</span>
            <span class="palette-path">{file.relativePath}</span>
          </button>
        {/each}
        {#if getQuickOpenResults().length === 0}
          <div class="palette-empty">No files found</div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- ── Command Palette ── -->
{#if commandPaletteOpen}
  <div class="palette-overlay" role="dialog" aria-modal="true" aria-label="Command Palette" onclick={() => { commandPaletteOpen = false; }} onkeydown={(e) => { if (e.key === 'Escape') commandPaletteOpen = false; }}>
    <div class="palette" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <input class="palette-input" type="text" bind:this={commandInputEl} bind:value={commandQuery} placeholder="> Type a command..." onkeydown={handleCommandKeydown} />
      <div class="palette-list">
        {#each getCommands() as cmd, i}
          <button class="palette-item" class:selected={i === commandSelectedIdx} onclick={() => executeCommand(cmd)}>
            <span class="palette-label">{cmd.label}</span>
            {#if cmd.shortcut}<span class="palette-shortcut">{cmd.shortcut}</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}

<!-- ── Keyboard Shortcuts overlay ── -->
{#if shortcutsVisible}
  <div class="palette-overlay" role="dialog" aria-modal="true" aria-label="Keyboard Shortcuts" onclick={() => { shortcutsVisible = false; }} onkeydown={(e) => { if (e.key === 'Escape') shortcutsVisible = false; }}>
    <div class="shortcuts-panel" role="document" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
      <div class="shortcuts-header">
        <span>Keyboard Shortcuts</span>
        <button class="icon-btn" onclick={() => { shortcutsVisible = false; }}>×</button>
      </div>
      <div class="shortcuts-body">
        <div class="shortcut-group">
          <h4>General</h4>
          <div class="shortcut-row"><span>Ctrl+Shift+P</span><span>Command Palette</span></div>
          <div class="shortcut-row"><span>Ctrl+P</span><span>Quick Open File</span></div>
          <div class="shortcut-row"><span>Ctrl+Shift+F</span><span>Search in Files</span></div>
          <div class="shortcut-row"><span>Ctrl+B</span><span>Toggle Sidebar</span></div>
          <div class="shortcut-row"><span>Ctrl+`</span><span>Toggle Output Panel</span></div>
          <div class="shortcut-row"><span>Ctrl+Shift+?</span><span>Keyboard Shortcuts</span></div>
        </div>
        <div class="shortcut-group">
          <h4>Editor</h4>
          <div class="shortcut-row"><span>Ctrl+S</span><span>Save File</span></div>
          <div class="shortcut-row"><span>Ctrl+W</span><span>Close Tab</span></div>
          <div class="shortcut-row"><span>Ctrl+Tab</span><span>Next Tab</span></div>
          <div class="shortcut-row"><span>Ctrl+Shift+Tab</span><span>Previous Tab</span></div>
          <div class="shortcut-row"><span>Ctrl+F</span><span>Find</span></div>
          <div class="shortcut-row"><span>Ctrl+H</span><span>Replace</span></div>
          <div class="shortcut-row"><span>Ctrl+G</span><span>Go to Line</span></div>
          <div class="shortcut-row"><span>Ctrl+D</span><span>Select Next Occurrence</span></div>
          <div class="shortcut-row"><span>Alt+Click</span><span>Multi-cursor</span></div>
          <div class="shortcut-row"><span>Ctrl+/</span><span>Toggle Comment</span></div>
          <div class="shortcut-row"><span>Shift+Alt+F</span><span>Format Document</span></div>
        </div>
        <div class="shortcut-group">
          <h4>Run</h4>
          <div class="shortcut-row"><span>F5</span><span>Run File</span></div>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  /* ═══════════════════════════════════════════════════════════════════════════ */
  /* IDE LAYOUT                                                                 */
  /* ═══════════════════════════════════════════════════════════════════════════ */

  .ide-layout {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }

  /* ── Sidebar ─────────────────────────────────────────────────────────────── */

  .sidebar {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--bg-panel);
    overflow: hidden;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.12em;
    color: var(--text-lo);
    padding: 10px 10px 6px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .sidebar-actions { display: flex; gap: 2px; }

  .icon-btn {
    background: none;
    border: none;
    color: var(--text-lo);
    cursor: pointer;
    padding: 3px 4px;
    border-radius: 3px;
    line-height: 0;
    display: flex;
    align-items: center;
    font-size: 10px;
    font-family: var(--font);
  }
  .icon-btn:hover { background: rgba(255,255,255,0.06); color: var(--text); }
  .icon-btn.active { color: var(--accent); background: rgba(217,119,6,0.1); }
  .icon-btn svg { display: block; }

  .resize-handle-v {
    width: 4px;
    cursor: col-resize;
    background: transparent;
    flex-shrink: 0;
    transition: background 0.15s;
  }
  .resize-handle-v:hover { background: var(--accent); }

  .resize-handle-h {
    height: 4px;
    cursor: row-resize;
    background: transparent;
    flex-shrink: 0;
    transition: background 0.15s;
  }
  .resize-handle-h:hover { background: var(--accent); }

  .tree-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 0;
  }

  .tree-scroll::-webkit-scrollbar { width: 6px; }
  .tree-scroll::-webkit-scrollbar-track { background: transparent; }
  .tree-scroll::-webkit-scrollbar-thumb { background: var(--border-hi); border-radius: 3px; }

  .tree-msg { font-size: 10px; color: var(--text-lo); padding: 12px; text-align: center; }

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
    padding: 1px 8px;
    line-height: 22px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tree-node:hover { background: rgba(255,255,255,0.03); }
  .tree-node.active { background: var(--accent-lo); }

  /* Git status colors */
  .tree-node.git-new .tree-name { color: #4ade80; }
  .tree-node.git-modified .tree-name { color: #fbbf24; }
  .tree-node.git-deleted .tree-name { color: #f87171; text-decoration: line-through; }

  .tree-chevron {
    font-size: 8px; color: var(--text-lo); width: 12px; text-align: center; flex-shrink: 0;
    transition: transform 0.12s; display: inline-block;
  }
  .tree-chevron.expanded { transform: rotate(90deg); }

  .tree-folder-icon { font-size: 10px; flex-shrink: 0; }
  .tree-file-icon { font-size: 9px; flex-shrink: 0; opacity: 0.6; width: 14px; text-align: center; }
  .tree-name { overflow: hidden; text-overflow: ellipsis; }
  .tree-file.active .tree-name { color: var(--accent); }

  /* ── Search panel ────────────────────────────────────────────────────────── */

  .search-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 8px;
    gap: 6px;
    overflow-y: auto;
  }

  .search-row {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .search-input {
    flex: 1;
    background: var(--bg-inset);
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text);
    font-family: var(--font);
    font-size: 11px;
    padding: 4px 8px;
    outline: none;
  }
  .search-input:focus { border-color: var(--accent); }

  .search-actions {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .search-go {
    background: var(--accent);
    border: none;
    color: #000;
    font-family: var(--font);
    font-size: 10px;
    font-weight: 600;
    padding: 3px 10px;
    border-radius: 3px;
    cursor: pointer;
  }
  .search-go:hover { opacity: 0.9; }

  .search-results {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .search-count {
    font-size: 9px;
    color: var(--text-lo);
    padding: 4px 0;
  }

  .search-result {
    display: flex;
    flex-direction: column;
    gap: 1px;
    background: none;
    border: none;
    text-align: left;
    padding: 4px 6px;
    border-radius: 3px;
    cursor: pointer;
    font-family: var(--font);
  }
  .search-result:hover { background: rgba(255,255,255,0.04); }

  .search-result-file {
    font-size: 10px;
    color: var(--accent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .search-result-text {
    font-size: 10px;
    color: var(--text-lo);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Main ────────────────────────────────────────────────────────────────── */

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  /* ── Tab bar ── */

  .tab-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    height: 34px;
    flex-shrink: 0;
  }

  .tab-bar-tabs {
    display: flex;
    align-items: center;
    overflow-x: auto;
    flex: 1;
    height: 100%;
  }
  .tab-bar-tabs::-webkit-scrollbar { height: 0; }

  .tab-bar-actions { display: flex; align-items: center; padding: 0 8px; flex-shrink: 0; gap: 6px; }

  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    background: none;
    border: none;
    border-right: 1px solid var(--border);
    font-family: var(--font);
    font-size: 11px;
    color: var(--text-lo);
    padding: 0 12px;
    height: 100%;
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    position: relative;
  }

  .tab:hover { background: rgba(255,255,255,0.02); }
  .tab.active { color: var(--text); background: var(--bg-inset); border-bottom: 2px solid var(--accent); }

  .tab-icon { font-size: 10px; opacity: 0.5; }
  .tab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); flex-shrink: 0; }

  .tab-close {
    background: none; border: none; color: var(--text-lo); font-size: 14px;
    line-height: 1; cursor: pointer; padding: 0 2px; border-radius: 2px; opacity: 0;
  }
  .tab:hover .tab-close { opacity: 0.5; }
  .tab-close:hover { opacity: 1 !important; background: rgba(255,255,255,0.1); }

  .run-btn {
    display: flex; align-items: center; gap: 4px;
    background: #22c55e18; border: 1px solid #22c55e30; color: #4ade80;
    font-family: var(--font); font-size: 10px; font-weight: 500;
    padding: 4px 10px; border-radius: 4px; cursor: pointer; white-space: nowrap;
  }
  .run-btn:hover:not(:disabled) { background: #22c55e28; border-color: #22c55e50; }
  .run-btn:disabled { opacity: 0.35; cursor: not-allowed; }
  .run-btn svg { display: block; }

  /* ── Breadcrumb ── */

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 12px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    font-size: 10px;
    color: var(--text-lo);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .breadcrumb-sep { color: var(--text-lo); opacity: 0.4; }
  .breadcrumb-item { cursor: default; }
  .breadcrumb-item:last-child { color: var(--text); }
  .breadcrumb-diff { color: var(--accent); font-weight: 600; }

  /* ── Editor area ── */

  .editor-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    position: relative;
  }

  .editor-container { flex: 1; min-height: 0; overflow: hidden; }
  .editor-container.hidden { display: none; }

  .editor-with-preview {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .editor-with-preview .editor-container {
    flex: 1;
    min-width: 0;
  }

  .markdown-preview {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 16px 24px;
    border-left: 1px solid var(--border);
    background: var(--bg-inset);
  }

  .markdown-body {
    font-size: 13px;
    line-height: 1.7;
    color: var(--text);
  }

  .markdown-body :global(h1) { font-size: 22px; margin: 16px 0 8px; color: var(--accent); }
  .markdown-body :global(h2) { font-size: 18px; margin: 14px 0 6px; color: var(--text); }
  .markdown-body :global(h3) { font-size: 15px; margin: 12px 0 4px; color: var(--text); }
  .markdown-body :global(code) { background: rgba(255,255,255,0.06); padding: 1px 4px; border-radius: 3px; font-size: 12px; }
  .markdown-body :global(pre) { background: rgba(0,0,0,0.3); padding: 12px; border-radius: 4px; overflow-x: auto; font-size: 12px; }
  .markdown-body :global(strong) { color: var(--text); }
  .markdown-body :global(a) { color: var(--accent); }
  .markdown-body :global(li) { margin: 2px 0; padding-left: 8px; }
  .markdown-body :global(hr) { border: none; border-top: 1px solid var(--border); margin: 12px 0; }

  .preview-container {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-inset);
    overflow: auto;
  }

  .image-preview {
    max-width: 90%;
    max-height: 90%;
    object-fit: contain;
    border-radius: 4px;
    box-shadow: 0 4px 20px rgba(0,0,0,0.3);
  }

  .empty-state {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px;
  }
  .empty-icon { margin-bottom: 8px; }
  .empty-title { color: var(--text-lo); font-size: 13px; }
  .empty-hint { color: var(--text-lo); font-size: 10px; opacity: 0.5; text-align: center; max-width: 300px; }
  .empty-shortcuts { margin-top: 16px; display: flex; flex-direction: column; gap: 4px; align-items: center; }
  .empty-shortcuts span { font-size: 9px; color: var(--text-lo); opacity: 0.4; letter-spacing: 0.05em; }

  /* ── Output panel ── */

  .output-panel {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
    background: var(--bg-panel);
  }

  .output-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 0 10px; height: 30px; flex-shrink: 0; border-bottom: 1px solid var(--border);
  }

  .output-tabs { display: flex; }
  .output-tab {
    background: none; border: none; color: var(--text-lo); font-family: var(--font);
    font-size: 9px; font-weight: 600; letter-spacing: 0.1em; padding: 7px 10px;
    cursor: pointer; border-bottom: 2px solid transparent;
  }
  .output-tab:hover { color: var(--text); }
  .output-tab.active { color: var(--accent); border-bottom-color: var(--accent); }

  .output-actions { display: flex; gap: 4px; }

  .output-body {
    flex: 1; overflow: auto; padding: 8px 12px; min-height: 0;
  }
  .output-body::-webkit-scrollbar { width: 6px; }
  .output-body::-webkit-scrollbar-track { background: transparent; }
  .output-body::-webkit-scrollbar-thumb { background: var(--border-hi); border-radius: 3px; }

  .output-content {
    margin: 0; font-family: var(--font); font-size: 11px; line-height: 1.6;
    color: var(--text); white-space: pre-wrap; word-break: break-all;
  }

  /* ── Status bar ── */

  .status-bar {
    display: flex; align-items: center; justify-content: space-between;
    height: 22px; padding: 0 10px; background: var(--bg-panel);
    border-top: 1px solid var(--border); flex-shrink: 0;
  }

  .status-left, .status-right { display: flex; align-items: center; gap: 12px; }

  .status-toggle {
    background: none; border: none; color: var(--text-lo); font-family: var(--font);
    font-size: 9px; cursor: pointer; padding: 0 4px; border-radius: 2px;
  }
  .status-toggle:hover { color: var(--text); background: rgba(255,255,255,0.06); }

  .status-item { font-size: 9px; color: var(--text-lo); }
  .status-btn { cursor: pointer; border: none; background: none; font-family: var(--font); }
  .status-btn:hover { color: var(--text); }
  .status-path { font-size: 9px; color: var(--text-lo); opacity: 0.7; }
  .status-flash { font-size: 9px; color: var(--accent); animation: flash-in 0.2s ease; }
  @keyframes flash-in { from { opacity: 0; } to { opacity: 1; } }

  /* ── Inline input ── */

  .inline-input { display: flex; align-items: center; gap: 4px; padding: 2px 0; }
  .inline-input input {
    flex: 1; background: var(--bg-inset); border: 1px solid var(--accent);
    border-radius: 3px; color: var(--text); font-family: var(--font);
    font-size: 11px; padding: 2px 6px; outline: none; line-height: 18px;
  }
  .inline-icon { font-size: 10px; flex-shrink: 0; }

  /* ── Context menu ── */

  .context-menu {
    position: fixed; z-index: 1000; background: var(--bg-panel);
    border: 1px solid var(--border-hi); border-radius: 5px; padding: 4px 0;
    min-width: 160px; box-shadow: 0 6px 20px rgba(0,0,0,0.5);
  }
  .context-menu button {
    display: block; width: 100%; background: none; border: none;
    color: var(--text); font-family: var(--font); font-size: 11px;
    padding: 5px 14px; text-align: left; cursor: pointer;
  }
  .context-menu button:hover { background: var(--accent-lo); }
  .context-menu .context-danger { color: var(--red); }
  .context-menu .context-danger:hover { background: #ef444420; }
  .context-sep { height: 1px; background: var(--border); margin: 4px 8px; }

  /* ── Command palette + Quick open ── */

  .palette-overlay {
    position: fixed; inset: 0; z-index: 2000; background: rgba(0,0,0,0.5);
    display: flex; align-items: flex-start; justify-content: center; padding-top: 80px;
  }

  .palette {
    width: 550px; max-height: 420px; background: var(--bg-panel);
    border: 1px solid var(--border-hi); border-radius: 6px;
    box-shadow: 0 8px 32px rgba(0,0,0,0.6); display: flex; flex-direction: column;
  }

  .palette-input {
    width: 100%; background: var(--bg-inset); border: none;
    border-bottom: 1px solid var(--border); border-radius: 6px 6px 0 0;
    color: var(--text); font-family: var(--font); font-size: 13px;
    padding: 12px 16px; outline: none;
  }

  .palette-list { flex: 1; overflow-y: auto; padding: 4px 0; }

  .palette-item {
    display: flex; align-items: center; gap: 8px;
    width: 100%; background: none; border: none; color: var(--text);
    font-family: var(--font); font-size: 12px; padding: 7px 16px;
    cursor: pointer; text-align: left;
  }
  .palette-item:hover, .palette-item.selected { background: var(--accent-lo); }

  .palette-icon { font-size: 11px; opacity: 0.5; flex-shrink: 0; }
  .palette-label { flex: 1; }
  .palette-path { font-size: 10px; color: var(--text-lo); opacity: 0.6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 250px; }
  .palette-shortcut { font-size: 10px; color: var(--text-lo); opacity: 0.6; }
  .palette-empty { padding: 12px 16px; font-size: 11px; color: var(--text-lo); text-align: center; }

  /* ── Keyboard shortcuts panel ── */

  .shortcuts-panel {
    width: 550px; max-height: 500px; background: var(--bg-panel);
    border: 1px solid var(--border-hi); border-radius: 6px;
    box-shadow: 0 8px 32px rgba(0,0,0,0.6); display: flex; flex-direction: column;
    overflow: hidden;
  }

  .shortcuts-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 12px 16px; border-bottom: 1px solid var(--border);
    font-size: 12px; font-weight: 600; color: var(--text);
  }

  .shortcuts-body {
    flex: 1; overflow-y: auto; padding: 12px 16px;
    display: flex; flex-direction: column; gap: 16px;
  }

  .shortcut-group h4 {
    font-size: 10px; font-weight: 600; color: var(--accent);
    letter-spacing: 0.1em; text-transform: uppercase; margin: 0 0 8px;
  }

  .shortcut-row {
    display: flex; justify-content: space-between; padding: 3px 0;
    font-size: 11px;
  }
  .shortcut-row span:first-child {
    color: var(--text-lo); background: rgba(255,255,255,0.04);
    padding: 1px 6px; border-radius: 3px; font-size: 10px;
  }
  .shortcut-row span:last-child { color: var(--text); }
</style>
