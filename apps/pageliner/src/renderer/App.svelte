<script>
  import { onMount } from 'svelte';
  import { TitleBar, AboutModal } from '@marina/desktop-ui/components';
  import { CommandPalette, commandRegistry } from '@marina/desktop-ui/command-palette';
  import { themeState } from '@marina/desktop-ui/theme';
  import SettingsModal from './components/SettingsModal.svelte';
  import SyncModal from './components/SyncModal.svelte';
  import LibraryGrid from './components/LibraryGrid.svelte';
  import LibrarySidebar from './components/LibrarySidebar.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import PdfReader from './components/PdfReader.svelte';
  import EpubReader from './components/EpubReader.svelte';
  import { libraryState } from './stores/library.svelte.js';

  const TITLEBAR_HEIGHT = '32px';

  let showAbout = $state(false);
  let showSettings = $state(false);
  let showSync = $state(false);
  let showPalette = $state(false);
  let appVersion = $state('2.2.5');

  // customTitlebarApplied is the startup value that decided whether the OS
  // frame is on; only it controls whether <TitleBar/> renders.
  let customTitlebarApplied = $state(false);

  // Layout toggles (persisted to UI prefs, like NoteLiner's layout state).
  let sidebarVisible = $state(true);
  let statusBarVisible = $state(true);

  // --- Commands ----------------------------------------------------------
  function registerCommands() {
    const C = (def) => commandRegistry.register(def);
    const ctrl = (e) => e.ctrlKey || e.metaKey;

    C({ id: 'library.import', label: 'Import Book…', section: 'Library', shortcut: 'Ctrl+O',
        matches: (e) => ctrl(e) && !e.shiftKey && !e.altKey && e.key === 'o',
        run: () => libraryState.importBooks() });
    C({ id: 'library.close', label: 'Close Book (Library)', section: 'Library',
        when: () => !!libraryState.selectedId, run: () => libraryState.clearSelection() });

    C({ id: 'view.toggleSidebar', label: 'Toggle Sidebar', section: 'View', shortcut: 'Ctrl+E',
        matches: (e) => ctrl(e) && !e.shiftKey && !e.altKey && e.key === 'e',
        run: () => toggleSidebar() });
    C({ id: 'view.toggleStatusBar', label: 'Toggle Status Bar', section: 'View', shortcut: 'Ctrl+J',
        matches: (e) => ctrl(e) && !e.shiftKey && !e.altKey && e.key === 'j',
        run: () => toggleStatusBar() });

    C({ id: 'app.sync', label: 'Git Sync', section: 'App', shortcut: 'Ctrl+Shift+S',
        matches: (e) => ctrl(e) && e.shiftKey && !e.altKey && e.code === 'KeyS',
        run: () => { showSync = true; } });
    C({ id: 'app.settings', label: 'Settings', section: 'App', shortcut: 'Ctrl+,',
        matches: (e) => ctrl(e) && !e.shiftKey && !e.altKey && e.key === ',',
        run: () => { showSettings = true; } });
    C({ id: 'app.about', label: 'About', section: 'App', shortcut: 'Ctrl+Shift+I',
        matches: (e) => ctrl(e) && e.shiftKey && !e.altKey && e.code === 'KeyI',
        run: () => { showAbout = true; } });
    C({ id: 'app.commandPalette', label: 'Command Palette', section: 'App', shortcut: 'Ctrl+K',
        matches: (e) => ctrl(e) && !e.altKey && !e.shiftKey && (e.key === 'k' || e.key === 'K'),
        run: () => { showPalette = true; } });
  }

  function openBook(id) {
    libraryState.select(id);
  }

  function toggleSidebar() {
    sidebarVisible = !sidebarVisible;
    window.api?.setUIPrefs?.({ sidebarVisible }).catch(() => {});
  }
  function toggleStatusBar() {
    statusBarVisible = !statusBarVisible;
    window.api?.setUIPrefs?.({ statusBarVisible }).catch(() => {});
  }

  // --- Keyboard ----------------------------------------------------------
  function handleKeydown(e) {
    if (showPalette) return; // palette owns keys while open
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey) {
      if (e.key === '=' || e.key === '+') { e.preventDefault(); themeState.zoomIn();    return; }
      if (e.key === '-' || e.key === '_') { e.preventDefault(); themeState.zoomOut();   return; }
      if (e.key === '0')                  { e.preventDefault(); themeState.zoomReset(); return; }
    }
    commandRegistry.dispatchKeyEvent(e);
  }

  // Ctrl+MouseWheel adjusts UI scale, throttled. Matches NoteLiner/ThreadLiner.
  let lastZoomTs = 0;
  const ZOOM_COOLDOWN_MS = 200;
  function handleWheel(e) {
    if (!(e.ctrlKey || e.metaKey)) return;
    if (e.deltaY === 0) return;
    e.preventDefault();
    const now = Date.now();
    if (now - lastZoomTs < ZOOM_COOLDOWN_MS) return;
    lastZoomTs = now;
    if (e.deltaY < 0) themeState.zoomIn();
    else themeState.zoomOut();
  }

  onMount(async () => {
    registerCommands();
    window.addEventListener('keydown', handleKeydown, true);
    window.addEventListener('wheel', handleWheel, { passive: false, capture: true });

    libraryState.load();

    if (window.api?.getUIPrefs) {
      try {
        const prefs = await window.api.getUIPrefs();
        customTitlebarApplied = !!prefs?.customTitlebar;
        if (typeof prefs?.sidebarVisible === 'boolean') sidebarVisible = prefs.sidebarVisible;
        if (typeof prefs?.statusBarVisible === 'boolean') statusBarVisible = prefs.statusBarVisible;
        document.documentElement.style.setProperty(
          '--titlebar-height',
          customTitlebarApplied ? TITLEBAR_HEIGHT : '0px',
        );
      } catch { /* non-critical */ }
    }
    try { appVersion = await window.api.getAppVersion(); } catch { /* keep default */ }

    return () => {
      window.removeEventListener('keydown', handleKeydown, true);
      window.removeEventListener('wheel', handleWheel, { capture: true });
    };
  });

  const selectedBook = $derived(libraryState.selectedBook);
  // The library sidebar is governed solely by the user's toggle — it stays
  // visible in reader mode too (alongside the reader's own Contents/Annotations
  // sidebar) and is only hidden when the user hides it (Ctrl+E / hamburger).
  const showLibrarySidebar = $derived(sidebarVisible);
</script>

{#snippet titlebarActions()}
  <button class="title-action" onclick={() => (showAbout = true)} title="About"><i class="fas fa-circle-info"></i></button>
  <button class="title-action" onclick={() => (showSettings = true)} title="Settings"><i class="fas fa-gear"></i></button>
{/snippet}

<div class="app-layout">
  {#if customTitlebarApplied}
    <TitleBar
      appName="PageLiner"
      onToggleToolbar={toggleSidebar}
      toolbarVisible={sidebarVisible}
      actions={titlebarActions}
    />
  {/if}

  <div class="app-body">
    {#if showLibrarySidebar}
      <aside class="app-sidebar"><LibrarySidebar /></aside>
    {/if}

    <div class="content">
      {#if selectedBook && selectedBook.format === 'pdf'}
        {#key selectedBook.id}
          <PdfReader book={selectedBook} onClose={() => libraryState.clearSelection()} />
        {/key}
      {:else if selectedBook && selectedBook.format === 'epub'}
        {#key selectedBook.id}
          <EpubReader book={selectedBook} onClose={() => libraryState.clearSelection()} />
        {/key}
      {:else if selectedBook}
        <main class="reader">
          <div class="reader-bar">
            <button class="back-btn" onclick={() => libraryState.clearSelection()}>
              <i class="fas fa-arrow-left"></i> Library
            </button>
            <div class="reader-title">{selectedBook.title}</div>
          </div>
          <div class="reader-body">
            <i class="fas fa-file-circle-question"></i>
            <h1>{selectedBook.title}</h1>
            <p>Unsupported format: {selectedBook.format.toUpperCase()}</p>
          </div>
        </main>
      {:else}
        <LibraryGrid onOpen={openBook} />
      {/if}
    </div>
  </div>

  {#if statusBarVisible}
    <StatusBar />
  {/if}
</div>

{#if showAbout}
  <AboutModal
    appName="PageLiner"
    version={appVersion}
    description="A desktop e-reader and document library built on the marina framework."
    repoUrl="https://github.com/NathanLaan/marina"
    repoLabel="github.com/NathanLaan/marina"
    iconClass="fa-book"
    onClose={() => (showAbout = false)}
  />
{/if}

{#if showSettings}
  <SettingsModal onClose={() => (showSettings = false)} />
{/if}

{#if showSync}
  <SyncModal onClose={() => (showSync = false)} />
{/if}

{#if showPalette}
  <CommandPalette onClose={() => (showPalette = false)} />
{/if}

<style>
  .app-layout {
    display: flex;
    flex-direction: column;
    zoom: var(--ui-zoom, 1);
    height: var(--ui-zoom-height, 100vh);
    width: var(--ui-zoom-width, 100vw);
    overflow: hidden;
  }

  .app-body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .app-sidebar {
    width: 240px;
    flex-shrink: 0;
    overflow: hidden;
    background: var(--bg-surface);
    border-right: 1px solid var(--border);
  }

  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    overflow: hidden;
    background: var(--bg-base);
  }

  .reader { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }

  .reader-bar {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .back-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-button);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
  }
  .back-btn:hover { background: var(--bg-button-hover); }
  .reader-title {
    font-size: 14px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .reader-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-muted);
    text-align: center;
    padding: 24px;
  }
  .reader-body i { font-size: 44px; opacity: 0.5; }
  .reader-body h1 { font-size: 22px; color: var(--text-primary); margin: 4px 0; }

  /* Matches the library .titlebar-btn look for actions injected via the
     <TitleBar actions={...}> snippet (the library scopes its own rules). */
  .title-action {
    -webkit-app-region: no-drag;
    width: 48px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent-on);
    opacity: 0.75;
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 12px;
    transition: background 0.15s, opacity 0.15s;
  }
  .title-action:hover { background: rgba(0, 0, 0, 0.18); opacity: 1; }
</style>
