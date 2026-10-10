<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getVersion } from "@tauri-apps/api/app";

  import {
    IconDownload,
    IconSettings,
    IconFolder,
    IconPlayerPlay,
    IconTrash,
    IconX,
    IconClipboard,
    IconVideo,
    IconMusic,
    IconCloudDownload,
    IconRadar,
    IconPlayerStop,
    IconRefresh,
    IconCheck,
    IconSearch,
    IconBrowser
  } from "@tabler/icons-svelte";

  import { t, getLocale, setLocale, initLocale, availableLocales } from '$lib/i18n.svelte';
  import { platforms, getServiceInfo } from '$lib/services';

  const RUNNING_STATUSES = ['downloading', 'analyzing', 'queued', 'merging'];
  const SETTLED_STATUSES = ['completed', 'failed', 'cancelled'];
  const TABS = ['all', 'downloading', 'completed', 'failed'] as const;
  type TabId = (typeof TABS)[number];

  const TOAST_TTL_MS = 8000;
  const TOAST_MAX = 3;

  // State declaration using Svelte 5 Runes
  let inputUrl = $state('');
  let extracting = $state(false);
  let isDragging = $state(false);
  let showSettings = $state(false);
  let inputMode = $state<'url' | 'sniffer'>('url');
  let submitting = $state(false);
  let appVersion = $state('');

  // Clipboard toasts are a queue: a second link must not silently replace the first one
  // the user may still be about to act on.
  let clipboardToasts = $state<{ id: number; url: string }[]>([]);
  let toastSeq = 0;
  let toastTimers: ReturnType<typeof setTimeout>[] = [];

  // Settings State
  let settings = $state({
    savePath: '',
    apiUrl: '',
    downloadMode: 'video',
    videoQuality: '720',
    audioFormat: 'best',
    clipboardMonitoring: true,
    maxParallelDownloads: 3,
    proxyEnabled: true,
    proxyUrl: 'http://127.0.0.1:7897',
    resumePartialDownloads: true,
    concurrentFragments: 4,
    downloadSubtitles: false,
    subtitleLangs: 'zh.*,en.*,en,zh-Hans,zh-Hant',
    embedSubtitles: true,
    embedMetadata: true,
    embedThumbnail: false,
    sponsorblockEnabled: false,
    sponsorblockMode: 'remove',
    playlistPrompt: true,
    notifyOnFinish: true,
    ytdlpAutoUpdate: true
  });
  // Guards against writing the placeholder defaults above back to disk if the user
  // touches a setting before the backend has answered.
  let settingsReady = $state(false);
  let bootError = $state('');
  let actionError = $state('');

  // Tasks List
  let tasks = $state<any[]>([]);
  let activeTab = $state<TabId>('all');
  let sniffer = $state<any>({ status: 'stopped', port: 8899, captures: [], message: null, supportedSources: [], certificateInstalled: false, proxyActive: false, wechatHooks: 0, tunProxyDetected: null });
  let snifferBusy = $state(false);
  let snifferReady = $state(false);
  let snifferError = $state('');
  let captureSearch = $state('');
  let thumbFailed = $state<Record<string, boolean>>({});

  // Playlist picker: one pending request (task id + episode list) at a time.
  let playlistRequest = $state<any>(null);
  let playlistSelection = $state<number[]>([]);
  let playlistBusy = $state(false);
  let playlistError = $state('');

  // Download engine (yt-dlp) status.
  let engine = $state<any>({ version: '', source: '', bundledVersion: '', autoUpdate: true, latest: '' });
  let engineBusy = $state(false);
  let engineLoading = $state(false);
  let engineError = $state('');
  let engineMessage = $state('');

  // Announcements for assistive tech; the visible list is silent on its own.
  let announcement = $state('');
  const lastStatus = new Map<string, string>();

  // Elements needed for focus management.
  let settingsPanel = $state<HTMLElement | null>(null);
  let settingsButton = $state<HTMLButtonElement | null>(null);
  let tabButtons = $state<Partial<Record<TabId, HTMLButtonElement | null>>>({});

  // Hoisted out of the filter callback — it was recomputed once per captured item.
  const captureQuery = $derived(captureSearch.trim().toLowerCase());
  let filteredCaptures = $derived(
    captureQuery
      ? sniffer.captures.filter((c: any) =>
          (c.title || '').toLowerCase().includes(captureQuery) || (c.source || '').toLowerCase().includes(captureQuery))
      : sniffer.captures
  );

  // Filter tasks based on active tab using Svelte 5 $derived rune
  let filteredTasks = $derived(
    activeTab === 'downloading'
      ? tasks.filter(task => RUNNING_STATUSES.includes(task.status))
      : activeTab === 'completed'
      ? tasks.filter(task => task.status === 'completed')
      : activeTab === 'failed'
      ? tasks.filter(task => ['failed', 'cancelled'].includes(task.status))
      : tasks
  );

  // Computed once per update instead of re-filtering the whole list inside the template.
  let tabCounts = $derived({
    all: tasks.length,
    downloading: tasks.filter(task => RUNNING_STATUSES.includes(task.status)).length,
    completed: tasks.filter(task => task.status === 'completed').length,
    failed: tasks.filter(task => ['failed', 'cancelled'].includes(task.status)).length
  });

  let hasSettledTasks = $derived(tasks.some(task => SETTLED_STATUSES.includes(task.status)));
  // "Nothing here yet" only makes sense when the list itself is empty; a filtered tab
  // needs different copy.
  let isFilteredEmpty = $derived(tasks.length > 0 && filteredTasks.length === 0);

  $effect(() => {
    if (showSettings) settingsPanel?.focus();
  });

  $effect(() => {
    for (const task of tasks) {
      const previous = lastStatus.get(task.id);
      if (previous === task.status) continue;
      lastStatus.set(task.id, task.status);
      // Only announce real transitions, not whatever state the app loaded with.
      if (previous !== undefined && SETTLED_STATUSES.includes(task.status)) {
        announcement = t(`status.announce.${task.status}`, { title: task.title });
      }
    }
    const live = new Set(tasks.map(task => task.id));
    for (const id of lastStatus.keys()) if (!live.has(id)) lastStatus.delete(id);
  });

  onMount(() => {
    initLocale();
    // A browser preview has no native backend; don't show a connection error.
    if (!isTauri()) return;
    let disposed = false;
    const unlisteners: (() => void)[] = [];

    void getVersion().then(version => { if (!disposed) appVersion = version; }).catch(() => {});
    void loadInitialState();

    async function loadInitialState() {
      // Register listeners BEFORE fetching initial state. If any of the invokes below
      // rejects, the app must not end up permanently deaf to task/clipboard/sniffer events.
      try {
        await Promise.all([
          listen('task-updated', (event) => {
            const updatedTask = event.payload as any;
            const index = tasks.findIndex(task => task.id === updatedTask.id);
            if (index !== -1) {
              tasks[index] = updatedTask;
              tasks = [...tasks]; // force Svelte 5 array proxy update
            } else {
              tasks = [updatedTask, ...tasks];
            }
          }),
          listen('clipboard-detected', (event) => {
            pushClipboardToast(event.payload as string);
          }),
          listen('sniffer-updated', (event) => {
            sniffer = event.payload as any;
          }),
          listen('playlist-choice', (event) => {
            openPlaylistRequest(event.payload as any);
          }),
          listen('engine-updated', () => {
            // A background engine update landed: show the new version.
            refreshEngine();
          }),
          listen('deep-link://new-url', (event) => {
            const urls = (event.payload as unknown as string[]) ?? [];
            for (const raw of urls) {
              const target = deepLinkTarget(raw);
              if (target) { handleDownload(target); }
            }
          }),
        ].map(subscription => subscription.then(unlisten => {
          if (disposed) unlisten();
          else unlisteners.push(unlisten);
        })));
      } catch (error) {
        if (!disposed) bootError = `Failed to connect to the backend: ${error}`;
        return;
      }

      if (disposed) return;

      // Load independently: proxy recovery must not delay settings or history.
      // Engine inspection is deferred until Preferences is opened.
      await Promise.all([
        invoke<typeof settings>('get_settings').then(value => { if (disposed) return; settings = value; settingsReady = true; })
          .catch(error => { if (!disposed) bootError = bootError || `Failed to load settings: ${error}`; }),
        invoke<any[]>('get_tasks').then(value => { if (disposed) return; tasks = value; })
          .catch(error => { if (!disposed) bootError = bootError || `Failed to load tasks: ${error}`; }),
        invoke('get_sniffer_state').then(value => { if (disposed) return; sniffer = value; snifferReady = true; })
          .catch(error => { if (!disposed) bootError = bootError || `Failed to load sniffer state: ${error}`; }),
      ]);
    }

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
    };
  });

  onDestroy(() => {
    for (const timer of toastTimers) clearTimeout(timer);
  });

  // --- Clipboard toasts -------------------------------------------------
  function dismissToast(id: number) {
    clipboardToasts = clipboardToasts.filter(toast => toast.id !== id);
  }

  function pushClipboardToast(url: string) {
    const clean = url?.trim();
    if (!clean || clipboardToasts.some(toast => toast.url === clean)) return;
    const id = ++toastSeq;
    clipboardToasts = [...clipboardToasts, { id, url: clean }].slice(-TOAST_MAX);
    toastTimers.push(setTimeout(() => dismissToast(id), TOAST_TTL_MS));
  }

  // --- Actions ----------------------------------------------------------
  /** Accept `cobalt://download?url=<encoded>` (browsers, Shortcuts, Alfred). */
  function deepLinkTarget(raw: string): string | null {
    const value = (raw ?? '').trim();
    if (!value) return null;
    if (/^https?:\/\//i.test(value)) return value;
    try {
      const parsed = new URL(value);
      const target = parsed.searchParams.get('url');
      return target && /^https?:\/\//i.test(target) ? target : null;
    } catch {
      return null;
    }
  }

  // --- Playlist episode picker -----------------------------------------
  function openPlaylistRequest(payload: any) {
    if (!payload?.taskId) return;
    playlistRequest = payload;
    playlistError = '';
    playlistSelection = (payload.entries ?? []).map((entry: any) => entry.index).filter((index: any) => typeof index === 'number');
  }

  function togglePlaylistEntry(index: number, checked: boolean) {
    playlistSelection = checked
      ? [...playlistSelection, index].sort((a, b) => a - b)
      : playlistSelection.filter((item: number) => item !== index);
  }

  function toggleAllPlaylistEntries(checked: boolean) {
    playlistSelection = checked
      ? (playlistRequest.entries ?? []).map((entry: any) => entry.index).filter((index: any) => typeof index === 'number')
      : [];
  }

  async function confirmPlaylistSelection(downloadAll = false) {
    if (!playlistRequest || playlistBusy) return;
    playlistBusy = true;
    playlistError = '';
    try {
      await invoke('resolve_playlist_choice', {
        taskId: playlistRequest.taskId,
        // An empty list means "every episode" (the picker only lists the first few).
        indices: downloadAll ? [] : playlistSelection
      });
      playlistRequest = null;
      playlistSelection = [];
    } catch (error) {
      playlistError = String(error);
    } finally {
      playlistBusy = false;
    }
  }

  function dismissPlaylist() {
    const pending = playlistRequest;
    playlistRequest = null;
    playlistSelection = [];
    if (!pending) return;
    // Cancelling the placeholder task frees its concurrency slot and stops the
    // backend from waiting for an answer that will never come.
    invoke('cancel_task', { id: pending.taskId }).catch(() => {});
  }

  // --- Download engine (yt-dlp) ----------------------------------------
  async function refreshEngine() {
    if (engineLoading || !isTauri()) return;
    engineLoading = true;
    try {
      engineError = '';
      engine = await invoke('get_engine_info');
    } catch (error) {
      engineError = String(error);
    } finally {
      engineLoading = false;
    }
  }

  async function updateEngine() {
    if (engineBusy) return;
    engineBusy = true;
    engineError = '';
    engineMessage = '';
    try {
      const info: any = await invoke('update_ytdlp_engine');
      engine = info;
      engineMessage = info?.justUpdated
        ? t('engine.updated', { version: info.version ?? '–' })
        : t('engine.up_to_date');
    } catch (error) {
      engineError = String(error);
    } finally {
      engineBusy = false;
    }
  }

  async function handleDownload(urlToDownload = inputUrl, toastId?: number) {
    const cleanUrl = (urlToDownload ?? '').trim();
    if (!cleanUrl || submitting) return;

    if (tasks.some(task => task.url === cleanUrl && RUNNING_STATUSES.includes(task.status))) {
      actionError = t('download.duplicate');
      return;
    }

    const previous = inputUrl;
    inputUrl = '';
    if (toastId !== undefined) dismissToast(toastId);
    submitting = true;

    try {
      await invoke('download_url', { url: cleanUrl });
      actionError = '';
    } catch (error) {
      inputUrl = previous; // never discard what the user actually typed
      actionError = String(error);
    } finally {
      submitting = false;
    }
  }

  async function handleManualExtract() {
    const cleanUrl = (inputUrl ?? '').trim();
    if (!cleanUrl || submitting || extracting) return;
    inputUrl = '';
    extracting = true;
    actionError = '';
    try {
      await invoke('start_manual_extraction', { url: cleanUrl });
    } catch (error) {
      actionError = String(error);
    } finally {
      extracting = false;
    }
  }

  async function startSniffer() {
    if (!snifferReady || snifferBusy) return;
    snifferBusy = true;
    snifferError = '';
    try { sniffer = await invoke('start_sniffer'); }
    catch (error) { snifferError = String(error); }
    finally { snifferBusy = false; }
  }

  // Failed tasks (often a login wall or an unknown site): jump to the resource
  // sniffer, the app's user-guided extraction fallback, and start it.
  async function openSnifferForGallery() {
    inputMode = 'sniffer';
    if (sniffer.status !== 'running' && sniffer.status !== 'starting' && !snifferBusy) {
      await startSniffer();
    }
  }

  async function stopSniffer() {
    snifferBusy = true;
    try { sniffer = await invoke('stop_sniffer'); }
    catch (error) { snifferError = String(error); }
    finally { snifferBusy = false; }
  }

  async function clearSniffer() {
    try { sniffer = await invoke('clear_sniffer_captures'); }
    catch (error) { snifferError = String(error); }
  }

  async function installSnifferCertificate() {
    snifferError = '';
    try { sniffer = await invoke('install_sniffer_certificate'); }
    catch (error) { snifferError = String(error); }
  }

  async function enableSnifferProxy() {
    snifferError = '';
    try { sniffer = await invoke('enable_sniffer_system_proxy'); }
    catch (error) { snifferError = String(error); }
  }

  async function restoreSnifferProxy() {
    snifferError = '';
    try { sniffer = await invoke('restore_sniffer_system_proxy'); }
    catch (error) { snifferError = String(error); }
  }

  async function downloadCapture(id: string) {
    snifferError = '';
    try { await invoke('download_captured_resource', { resourceId: id }); }
    catch (error) { snifferError = String(error); }
  }

  async function selectDirectory() {
    try {
      const path: string | null = await invoke('select_directory');
      if (path) {
        settings.savePath = path;
        await saveSettings();
      }
    } catch (error) {
      actionError = String(error);
    }
  }

  async function saveSettings() {
    if (!settingsReady) return;
    const payload = JSON.parse(JSON.stringify(settings));
    // Whitespace here silently breaks the Rust-side proxy parse, which used to downgrade
    // to a direct connection without telling anyone.
    payload.apiUrl = String(payload.apiUrl ?? '').trim();
    payload.proxyUrl = String(payload.proxyUrl ?? '').trim();
    // A <select> hands back strings; the Rust side types this as u32.
    const fragments = Number(payload.concurrentFragments);
    payload.concurrentFragments = Number.isFinite(fragments) ? Math.min(16, Math.max(1, Math.round(fragments))) : 4;
    try {
      settings = await invoke('save_settings', { newSettings: payload });
      actionError = '';
    } catch (error) {
      actionError = String(error);
    }
  }

  async function cancelTask(id: string) {
    try {
      await invoke('cancel_task', { id });
      actionError = '';
    } catch (error) {
      actionError = String(error);
    }
  }

  async function deleteTask(id: string) {
    try {
      await invoke('delete_task', { id });
      tasks = tasks.filter(task => task.id !== id);
      actionError = '';
    } catch (error) {
      actionError = String(error);
    }
  }

  async function clearCompleted() {
    try {
      tasks = await invoke('clear_completed');
      actionError = '';
    } catch (error) {
      actionError = String(error);
    }
  }

  async function revealInFinder(path: string) {
    if (!path) return;
    try { await invoke('reveal_in_finder', { path }); }
    catch (error) { actionError = String(error); }
  }

  async function openFile(path: string) {
    if (!path) return;
    try { await invoke('open_file', { path }); }
    catch (error) { actionError = String(error); }
  }

  // --- Settings panel focus management ----------------------------------
  function openSettings() {
    actionError = '';
    showSettings = true;
    void refreshEngine();
  }

  function closeSettings() {
    showSettings = false;
    settingsButton?.focus();
  }

  function handleSettingsKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation();
      closeSettings();
      return;
    }
    if (event.key !== 'Tab' || !settingsPanel) return;

    const focusable = settingsPanel.querySelectorAll<HTMLElement>(
      'button, input, select, textarea, a[href], [tabindex]:not([tabindex="-1"])'
    );
    if (focusable.length === 0) return;

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;

    // Keep Tab cycling inside the dialog instead of escaping to the page behind it.
    if (event.shiftKey && (active === first || active === settingsPanel)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function moveTab(delta: number) {
    const current = TABS.indexOf(activeTab);
    activeTab = TABS[(current + delta + TABS.length) % TABS.length];
    queueMicrotask(() => tabButtons[activeTab]?.focus());
  }

  // --- Drag & drop ------------------------------------------------------
  // A plain dragleave fires whenever the pointer crosses a child element, which made the
  // overlay flicker. Count enter/leave pairs instead.
  let dragDepth = 0;

  function handleDragEnter(e: DragEvent) {
    e.preventDefault();
    dragDepth += 1;
    isDragging = true;
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    dragDepth = 0;
    isDragging = false;

    const text = e.dataTransfer?.getData('text')?.trim();
    if (text) {
      handleDownload(text);
      return;
    }
    // Dropping a file used to do nothing at all, which reads as a broken app.
    if ((e.dataTransfer?.files?.length ?? 0) > 0) {
      actionError = t('drop.file_unsupported');
    }
  }

  // --- Formatting -------------------------------------------------------
  function formatBytes(bytes: number, decimals = 2) {
    if (!bytes || bytes <= 0) return t('unit.zero');
    const k = 1024;
    const units = ['unit.bytes', 'unit.kb', 'unit.mb', 'unit.gb', 'unit.tb'];
    const dm = decimals < 0 ? 0 : decimals;
    const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), units.length - 1);
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + t(units[i]);
  }

  // t() degrades to the raw key when a locale is missing an entry, so fall back to the
  // platform's own name rather than showing "platform.xinpianchang" to the user.
  function platformName(id: string) {
    const key = `platform.${id}`;
    const label = t(key);
    return label === key ? (platforms.find(platform => platform.id === id)?.fallbackName ?? id) : label;
  }

  function humanTaskError(error?: string) {
    const raw = error || '';
    const lower = raw.toLowerCase();
    if (lower.includes('youtube') && (lower.includes('cookies expired') || lower.includes('login cookies'))) {
      return t('error.youtube_cookies');
    }
    if (lower.includes('401 unauthorized')) return t('error.unauthorized');
    if (lower.includes('502 bad gateway')) return t('error.media_gateway');
    if (lower.includes('media server error')) return t('error.media_server');
    return raw || t('error.unknown');
  }
</script>

<!-- Drag & Drop overlay -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<main
  class="app-container"
  ondragenter={handleDragEnter}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
>
  {#if isDragging}
    <div class="drop-overlay" transition:fade={{ duration: 160 }}>
      <div class="drop-card">
        <IconCloudDownload size={64} color="var(--accent-primary)" />
        <h2>{t('drop.title')}</h2>
        <p>{t('drop.subtitle')}</p>
      </div>
    </div>
  {/if}

  <!-- Header / Window Bar -->
  <header class="window-header drag-handle" data-tauri-drag-region>
    <div class="header-title no-drag">
      <span class="gradient-text">COBALT</span>
    </div>
    <div class="header-actions no-drag">
      <button
        class="settings-btn"
        bind:this={settingsButton}
        onclick={openSettings}
        title={t('settings.title')}
        aria-label={t('settings.title')}
        aria-haspopup="dialog"
        aria-expanded={showSettings}
      >
        <IconSettings size={18} />
      </button>
    </div>
  </header>

  <div class="workspace" class:capture-mode={inputMode === 'sniffer'}>
    <div
      class="mode-switch"
      role="tablist"
      tabindex="-1"
      aria-label={t('sniffer.mode_label')}
      onkeydown={(e) => {
        if (e.key === 'ArrowRight') { e.preventDefault(); inputMode = inputMode === 'url' ? 'sniffer' : 'url'; }
        if (e.key === 'ArrowLeft') { e.preventDefault(); inputMode = inputMode === 'sniffer' ? 'url' : 'sniffer'; }
      }}
    >
      <button
        role="tab"
        class:active={inputMode === 'url'}
        aria-selected={inputMode === 'url'}
        tabindex={inputMode === 'url' ? 0 : -1}
        onclick={() => inputMode = 'url'}
      ><IconDownload size={15} />{t('sniffer.mode_url')}</button>
      <button
        role="tab"
        class:active={inputMode === 'sniffer'}
        aria-selected={inputMode === 'sniffer'}
        tabindex={inputMode === 'sniffer' ? 0 : -1}
        onclick={() => inputMode = 'sniffer'}
      ><IconRadar size={15} />{t('sniffer.mode_capture')}</button>
    </div>

    {#if bootError || actionError}
      <p class="app-banner" role="alert">{bootError || actionError}</p>
    {/if}

    <!-- URL Paste Section -->
    {#if inputMode === 'url'}
    <section class="paste-section">
      <div class="input-glow-wrapper">
        <input
          type="text"
          aria-label={t('input.placeholder')}
          placeholder={t('input.placeholder')}
          bind:value={inputUrl}
          onkeydown={(e) => e.key === 'Enter' && handleDownload()}
          autocapitalize="off"
          autocomplete="off"
          spellcheck="false"
          class="url-input"
        />
        <button class="download-trigger-btn" onclick={() => handleDownload()} disabled={!inputUrl.trim() || submitting}>
          <IconDownload size={18} />
          <span>{submitting ? t('download.submitting') : t('analyze')}</span>
        </button>
        <button
          class="manual-extract-btn"
          onclick={() => handleManualExtract()}
          disabled={!inputUrl.trim() || submitting || extracting}
          title={t('home.manual_extract')}
        >
          <IconBrowser size={16} />
          <span>{extracting ? t('download.submitting') : t('home.manual_extract')}</span>
        </button>
      </div>
    </section>
    {:else}
    <section class="sniffer-section">
      <div class="sniffer-heading">
        <div>
          <h2>{t('sniffer.title')}</h2>
          <p>{t('sniffer.description')}</p>
        </div>
        {#if sniffer.status === 'running' || sniffer.status === 'starting'}
          <button class="sniffer-control stop" onclick={stopSniffer} disabled={snifferBusy}><IconPlayerStop size={16} />{t('sniffer.stop')}</button>
        {:else}
          <button class="sniffer-control" onclick={startSniffer} disabled={snifferBusy || !snifferReady}><IconRadar size={16} />{t('sniffer.start')}</button>
        {/if}
      </div>
      <div class="sniffer-notice">
        <strong>{t('sniffer.proxy_title')}</strong>
        <span>{t('sniffer.proxy_note', { port: sniffer.port })}</span>
        {#if sniffer.tunProxyDetected}
          <p class="sniffer-tun-warning">{t('sniffer.tun_warning', { app: sniffer.tunProxyDetected })}</p>
        {/if}
        <div class="sniffer-setup-actions">
          <button class:complete={sniffer.certificateInstalled} onclick={installSnifferCertificate} disabled={sniffer.certificateInstalled || snifferBusy || !snifferReady}>
            {#if sniffer.certificateInstalled}<IconCheck size={14} />{/if}
            {sniffer.certificateInstalled ? t('sniffer.certificate_installed') : t('sniffer.install_certificate')}
          </button>
          {#if sniffer.proxyActive}
            <button class="complete" onclick={restoreSnifferProxy} disabled={snifferBusy}><IconCheck size={14} />{t('sniffer.proxy_enabled')}</button>
          {:else}
            <button onclick={enableSnifferProxy} disabled={sniffer.status !== 'running' || snifferBusy || !!sniffer.tunProxyDetected}>{t('sniffer.enable_proxy')}</button>
          {/if}
        </div>
      </div>
      {#if snifferError}
        <p class="sniffer-error">{snifferError}</p>
      {:else if sniffer.message}
        <p class="sniffer-message">{sniffer.message}</p>
      {/if}
      <div class="sniffer-capture-header">
        <span>{t('sniffer.captures')} <b>{sniffer.captures.length}</b></span>
        <div class="capture-header-tools">
          {#if sniffer.status === 'running'}
            <span class:ready={sniffer.proxyActive && sniffer.wechatHooks > 0} class="sniffer-hook-status">
              {!sniffer.proxyActive ? t('sniffer.waiting_proxy') : sniffer.wechatHooks > 0 ? t('sniffer.hook_ready') : t('sniffer.waiting_hook')}
            </span>
          {/if}
          <input class="capture-search" type="search" aria-label={t('sniffer.search_placeholder')} placeholder={t('sniffer.search_placeholder')} bind:value={captureSearch} />
          <button class="capture-clear" onclick={clearSniffer} disabled={sniffer.captures.length === 0} title={t('sniffer.clear')} aria-label={t('sniffer.clear')}><IconRefresh size={15} /></button>
        </div>
      </div>
      {#if sniffer.captures.length === 0}
        <div class="sniffer-empty"><IconRadar size={28} /><span>{t('sniffer.empty')}</span></div>
      {:else if filteredCaptures.length === 0}
        <div class="sniffer-empty"><IconSearch size={24} /><span>{t('sniffer.no_match')}</span></div>
      {:else}
        <div class="capture-grid">
          {#each filteredCaptures as capture (capture.id)}
            <div class="capture-card">
              <div class="capture-thumb">
                {#if capture.coverUrl && !thumbFailed[capture.coverUrl]}
                  <img src={capture.coverUrl} alt="" loading="lazy" referrerpolicy="no-referrer" onerror={() => (thumbFailed[capture.coverUrl] = true)} />
                {:else}
                  <div class="capture-thumb-fallback"><IconVideo size={22} /></div>
                {/if}
                <span class="capture-kind-pill">{capture.kind === 'playlist' ? 'HLS' : capture.kind.toUpperCase()}</span>
              </div>
              <div class="capture-info">
                <strong title={capture.title}>{capture.title}</strong>
                <span>{capture.source} · {capture.size > 0 ? formatBytes(capture.size) : t('task.unknown_size')}</span>
              </div>
              <button class="capture-download" onclick={() => downloadCapture(capture.id)} disabled={capture.kind === 'playlist'}>
                <IconDownload size={15} />{capture.kind === 'playlist' ? 'HLS' : t('sniffer.download')}
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </section>
    {/if}

    <!-- Tabs Navigation -->
    <nav class="tabs-nav" aria-label={t('tabs.label')}>
      <div class="tabs-list" role="tablist" tabindex="-1" onkeydown={(e) => {
        if (e.key === 'ArrowRight') { e.preventDefault(); moveTab(1); }
        else if (e.key === 'ArrowLeft') { e.preventDefault(); moveTab(-1); }
        else if (e.key === 'Home') { e.preventDefault(); activeTab = TABS[0]; }
        else if (e.key === 'End') { e.preventDefault(); activeTab = TABS[TABS.length - 1]; }
      }}>
        {#each TABS as tab}
          <button
            class="tab-btn"
            class:active={activeTab === tab}
            role="tab"
            aria-selected={activeTab === tab}
            tabindex={activeTab === tab ? 0 : -1}
            bind:this={tabButtons[tab]}
            onclick={() => activeTab = tab}
          >
            {t(`tabs.${tab}`)} <span class="tab-count">{tabCounts[tab]}</span>
          </button>
        {/each}
      </div>
      {#if hasSettledTasks}
        <button class="clear-btn" onclick={clearCompleted}>
          {t('tabs.clear_finished')}
        </button>
      {/if}
    </nav>

    <!-- Downloads List Area -->
    <section class="downloads-area">
      {#if filteredTasks.length === 0}
        <div class="empty-state">
          <div class="empty-icon-pulse">
            <IconSearch size={40} color="var(--text-muted)" />
          </div>
          {#if isFilteredEmpty}
            <h3>{t('empty.filtered.title')}</h3>
            <p>{t('empty.filtered.subtitle')}</p>
          {:else}
            <h3>{t('empty.title')}</h3>
            <p>{t('empty.subtitle')}</p>
          {/if}
        </div>
      {:else}
        <div class="tasks-list">
          {#each filteredTasks as task (task.id)}
            {@const service = getServiceInfo(task.url, t('service.unknown'), platformName)}
            <div class="task-card glass">
              <div class="service-icon" style="--service-bg: {service.bg}; --service-color: {service.color}">
                {#if settings.downloadMode === 'audio'}
                  <IconMusic size={18} />
                {:else}
                  <IconVideo size={18} />
                {/if}
              </div>

              <!-- Main Info Column -->
              <div class="task-info-col">
                <div class="task-header">
                  <div class="task-title-group">
                    <span class="task-title" title={task.title}>{task.title}</span>
                    <span class="task-service" style="color: {service.color}">{service.name}</span>
                  </div>
                  <span class="task-status-badge {task.status}">{t(`task.status.${task.status}`)}</span>
                </div>

                <!-- Progress bar -->
                <div class="progress-bar-wrapper">
                  <div
                    class="progress-bar-bg"
                    role="progressbar"
                    aria-label={t('progress.label')}
                    aria-valuenow={Math.round(task.progress * 100)}
                    aria-valuemin="0"
                    aria-valuemax="100"
                  >
                    <div
                      class="progress-bar-fill {task.status}"
                      class:indeterminate={task.status === 'downloading' && task.kind !== 'gallery' && task.totalBytes === 0}
                      style="width: {task.progress * 100}%"
                    ></div>
                  </div>
                </div>

                <!-- Status line -->
                <div class="task-status-footer">
                  {#if task.status === 'queued'}
                    <span class="stats-text">{t('task.waiting_slot')}</span>
                  {:else if task.status === 'downloading'}
                    {#if task.kind === 'gallery'}
                      <span class="stats-text">{t('task.gallery_items', { done: task.itemsDone, total: task.itemsTotal > 0 ? task.itemsTotal : '…' })}</span>
                    {:else}
                      <span class="stats-text">
                        {formatBytes(task.downloadedBytes)} / {task.totalBytes > 0 ? formatBytes(task.totalBytes) : t('task.unknown_size')}
                      </span>
                      <span class="stats-text speed">{task.speed}</span>
                      <span class="stats-text eta">{t('task.eta')}: {task.eta}</span>
                    {/if}
                  {:else if task.status === 'analyzing'}
                    {#if task.kind === 'gallery'}
                      <span class="stats-text animated-dots">{t('task.gallery_analyzing')}</span>
                    {:else}
                      <span class="stats-text animated-dots">{t('task.connecting')}</span>
                    {/if}
                  {:else if task.status === 'merging'}
                    <span class="stats-text animated-dots font-semibold text-indigo-400">{t('task.merging')}</span>
                  {:else if task.status === 'completed'}
                    {#if task.kind === 'gallery'}
                      <span class="stats-text success">{t('task.gallery_done', { total: task.itemsTotal })}</span>
                    {:else}
                      <span class="stats-text success">{t('task.completed')}</span>
                    {/if}
                  {:else if task.status === 'failed'}
                    <span class="stats-text error" title={task.error}>{humanTaskError(task.error)}</span>
                  {:else if task.status === 'cancelled'}
                    <span class="stats-text warning">{t('task.cancelled')}</span>
                  {/if}
                </div>
              </div>

              <!-- Actions Column -->
              <div class="task-actions-col">
                {#if RUNNING_STATUSES.includes(task.status)}
                  <button class="action-circle-btn danger" onclick={() => cancelTask(task.id)} title={t('action.cancel')} aria-label={t('action.cancel')}>
                    <IconX size={14} />
                  </button>
                {:else if task.status === 'completed'}
                  <button class="action-circle-btn success" onclick={() => openFile(task.outputPath)} title={t('action.play')} aria-label={t('action.play')}>
                    <IconPlayerPlay size={14} />
                  </button>
                  <button class="action-circle-btn secondary" onclick={() => revealInFinder(task.outputPath)} title={t('action.reveal')} aria-label={t('action.reveal')}>
                    <IconFolder size={14} />
                  </button>
                  <button class="action-circle-btn secondary" onclick={() => deleteTask(task.id)} title={t('action.remove')} aria-label={t('action.remove')}>
                    <IconTrash size={14} />
                  </button>
                {:else}
                  {#if task.kind === 'gallery' || task.status === 'failed'}
                    <button class="action-circle-btn" onclick={openSnifferForGallery} title={t('task.use_sniffer')} aria-label={t('task.use_sniffer')}>
                      <IconRadar size={14} />
                    </button>
                  {/if}
                  {#if !task.url.startsWith('capture://')}
                    <button class="action-circle-btn" onclick={() => handleDownload(task.url)} title={t('action.retry')} aria-label={t('action.retry')}>
                      <IconRefresh size={14} />
                    </button>
                  {/if}
                  <button class="action-circle-btn secondary" onclick={() => deleteTask(task.id)} title={t('action.remove')} aria-label={t('action.remove')}>
                    <IconTrash size={14} />
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  </div>
  <!-- Screen-reader announcements for task state changes -->
  <div class="sr-only" aria-live="polite" aria-atomic="true">{announcement}</div>

  <!-- Clipboard Toast Stack -->
  <div class="toast-stack">
    {#each clipboardToasts as toast (toast.id)}
      <div class="clipboard-toast glass" in:fly={{ y: 12, duration: 220 }} out:fly={{ y: -8, duration: 140 }}>
        <div class="toast-content">
          <IconClipboard size={20} color="var(--accent-primary)" />
          <div class="toast-text">
            <h4>{t('toast.detected')}</h4>
            <p class="truncate">{toast.url}</p>
          </div>
        </div>
        <div class="toast-actions">
          <button class="toast-btn secondary" onclick={() => dismissToast(toast.id)}>{t('toast.ignore')}</button>
          <button class="toast-btn primary" onclick={() => handleDownload(toast.url, toast.id)}>{t('toast.download')}</button>
        </div>
      </div>
    {/each}
  </div>

  <!-- Settings Slide Panel -->
  {#if showSettings}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="settings-backdrop" onclick={closeSettings} transition:fade={{ duration: 160 }}>
      <div
        class="settings-panel glass"
        role="dialog"
        aria-modal="true"
        aria-label={t('settings.title')}
        tabindex="-1"
        bind:this={settingsPanel}
        onclick={(e) => e.stopPropagation()}
        onkeydown={handleSettingsKeydown}
        in:fly={{ x: 24, duration: 220 }}
        out:fly={{ x: 16, duration: 140 }}
      >
        <div class="settings-header">
          <h3>{t('settings.title')}</h3>
          <button class="close-settings" onclick={closeSettings} title={t('settings.close')} aria-label={t('settings.close')}>
            <IconX size={18} />
          </button>
        </div>

        <div class="settings-body">
          <!-- Language -->
          <div class="setting-item">
            <label for="lang-select">{t('settings.language')}</label>
            <select id="lang-select" value={getLocale()} onchange={(e) => setLocale(e.currentTarget.value)} class="settings-select">
              {#each availableLocales as locale (locale.code)}
                <option value={locale.code}>{locale.label}</option>
              {/each}
            </select>
          </div>

          <!-- Save Path Option -->
          <div class="setting-item">
            <label for="save-path">{t('settings.save_folder')}</label>
            <div class="path-selector">
              <input type="text" readonly value={settings.savePath} id="save-path" class="path-input" />
              <button class="btn-select-dir" onclick={selectDirectory} title={t('settings.save_folder')} aria-label={t('settings.save_folder')}>
                <IconFolder size={16} />
              </button>
            </div>
          </div>

          <!-- Download Mode -->
          <div class="setting-item">
            <label for="download-mode">{t('settings.download_mode')}</label>
            <select id="download-mode" bind:value={settings.downloadMode} onchange={saveSettings} class="settings-select">
              <option value="video">{t('settings.video_audio')}</option>
              <option value="audio">{t('settings.audio_only')}</option>
            </select>
          </div>

          {#if settings.downloadMode === 'video'}
            <!-- Video Quality -->
            <div class="setting-item">
              <label for="video-quality">{t('settings.video_quality')}</label>
              <select id="video-quality" bind:value={settings.videoQuality} onchange={saveSettings} class="settings-select">
                <option value="max">{t('settings.quality.max')}</option>
                <option value="1080">{t('settings.quality.1080')}</option>
                <option value="720">{t('settings.quality.720')}</option>
                <option value="480">{t('settings.quality.480')}</option>
                <option value="360">{t('settings.quality.360')}</option>
              </select>
            </div>
          {:else}
            <!-- Audio Format -->
            <div class="setting-item">
              <label for="audio-format">{t('settings.audio_format')}</label>
              <select id="audio-format" bind:value={settings.audioFormat} onchange={saveSettings} class="settings-select">
                <option value="best">{t('settings.audio.best')}</option>
                <option value="mp3">{t('settings.audio.mp3')}</option>
                <option value="ogg">{t('settings.audio.ogg')}</option>
                <option value="wav">{t('settings.audio.wav')}</option>
                <option value="opus">{t('settings.audio.opus')}</option>
              </select>
            </div>
          {/if}

          <!-- Clipboard monitor -->
          <div class="setting-item checkbox-item">
            <input type="checkbox" id="clip-monitor" bind:checked={settings.clipboardMonitoring} onchange={saveSettings} />
            <label for="clip-monitor">{t('settings.clipboard')}</label>
          </div>

          <!-- Download engine -->
          <div class="setting-divider"></div>
          <div class="setting-section-title">{t('settings.engine')}</div>

          <div class="setting-item engine-item">
            <div class="engine-status">
              <span class="engine-version">{engineLoading ? t('engine.loading') : t('engine.version', { version: engine.version || '–' })}</span>
              {#if !engineLoading && engine.source}
                <span class="engine-source {engine.source}">{t(`engine.source.${engine.source}`)}</span>
              {/if}
            </div>
            <div class="engine-actions">
              <button class="btn-select-dir" onclick={updateEngine} disabled={engineBusy || engineLoading} title={t('engine.check_update')} aria-label={t('engine.check_update')}>
                <IconRefresh size={16} />
              </button>
            </div>
          </div>
          <div class="setting-item">
            <span class="setting-hint">
              {engineMessage || engineError || t('engine.note', { bundled: engine.bundledVersion || '–' })}
            </span>
          </div>
          <div class="setting-item checkbox-item">
            <input type="checkbox" id="engine-auto" bind:checked={settings.ytdlpAutoUpdate} onchange={saveSettings} />
            <label for="engine-auto">{t('engine.auto_update')}</label>
          </div>

          <!-- Download behaviour -->
          <div class="setting-divider"></div>
          <div class="setting-section-title">{t('settings.behavior')}</div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="resume-partial" bind:checked={settings.resumePartialDownloads} onchange={saveSettings} />
            <label for="resume-partial">{t('settings.resume')}</label>
          </div>

          <div class="setting-item">
            <label for="fragments">{t('settings.fragments')}</label>
            <select id="fragments" bind:value={settings.concurrentFragments} onchange={saveSettings} class="settings-select">
              <option value={1}>{t('settings.fragments.1')}</option>
              <option value={2}>2</option>
              <option value={4}>4</option>
              <option value={8}>8</option>
            </select>
            <span class="setting-hint">{t('settings.fragments.hint')}</span>
          </div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="playlist-prompt" bind:checked={settings.playlistPrompt} onchange={saveSettings} />
            <label for="playlist-prompt">{t('settings.playlist_prompt')}</label>
          </div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="notify-finish" bind:checked={settings.notifyOnFinish} onchange={saveSettings} />
            <label for="notify-finish">{t('settings.notify')}</label>
          </div>

          <!-- Subtitles & metadata -->
          <div class="setting-divider"></div>
          <div class="setting-section-title">{t('settings.subtitles')}</div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="download-subs" bind:checked={settings.downloadSubtitles} onchange={saveSettings} />
            <label for="download-subs">{t('settings.download_subtitles')}</label>
          </div>

          {#if settings.downloadSubtitles}
            <div class="setting-item">
              <label for="sub-langs">{t('settings.subtitle_langs')}</label>
              <input type="text" id="sub-langs" bind:value={settings.subtitleLangs} onchange={saveSettings} class="settings-input" placeholder={t('settings.subtitle_langs.hint')} />
              <span class="setting-hint">{t('settings.subtitle_langs.note')}</span>
            </div>
            <div class="setting-item checkbox-item">
              <input type="checkbox" id="embed-subs" bind:checked={settings.embedSubtitles} onchange={saveSettings} />
              <label for="embed-subs">{t('settings.embed_subtitles')}</label>
            </div>
          {/if}

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="embed-metadata" bind:checked={settings.embedMetadata} onchange={saveSettings} />
            <label for="embed-metadata">{t('settings.embed_metadata')}</label>
          </div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="embed-thumbnail" bind:checked={settings.embedThumbnail} onchange={saveSettings} />
            <label for="embed-thumbnail">{t('settings.embed_thumbnail')}</label>
          </div>

          <!-- SponsorBlock (YouTube) -->
          <div class="setting-item checkbox-item">
            <input type="checkbox" id="sponsorblock" bind:checked={settings.sponsorblockEnabled} onchange={saveSettings} />
            <label for="sponsorblock">{t('settings.sponsorblock')}</label>
          </div>

          {#if settings.sponsorblockEnabled}
            <div class="setting-item">
              <select id="sponsorblock-mode" bind:value={settings.sponsorblockMode} onchange={saveSettings} class="settings-select">
                <option value="remove">{t('settings.sponsorblock.remove')}</option>
                <option value="mark">{t('settings.sponsorblock.mark')}</option>
              </select>
              <span class="setting-hint">{t('settings.sponsorblock.hint')}</span>
            </div>
          {/if}

          <!-- Remote API -->
          <div class="setting-divider"></div>
          <div class="setting-section-title">{t('settings.api')}</div>

          <div class="setting-item">
            <label for="api-url">{t('settings.api.url')}</label>
            <input type="text" id="api-url" bind:value={settings.apiUrl} onchange={saveSettings} class="settings-input" placeholder={t('settings.api.hint')} />
            <span class="setting-hint">{t('settings.api.note')}</span>
          </div>

          <!-- Proxy Settings -->
          <div class="setting-divider"></div>
          <div class="setting-section-title">{t('settings.proxy')}</div>

          <div class="setting-item checkbox-item">
            <input type="checkbox" id="proxy-enable" bind:checked={settings.proxyEnabled} onchange={saveSettings} />
            <label for="proxy-enable">{t('settings.proxy.enable')}</label>
          </div>

          {#if settings.proxyEnabled}
            <div class="setting-item">
              <label for="proxy-url">{t('settings.proxy.url')}</label>
              <input type="text" id="proxy-url" bind:value={settings.proxyUrl} onchange={saveSettings} class="settings-input" placeholder={t('settings.proxy.hint')} />
              <span class="setting-hint">{t('settings.proxy.note')}</span>
            </div>
          {/if}
        </div>

        <div class="settings-footer">
          <p class="settings-app-version">{t('settings.version', { version: appVersion || '–' })}</p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Playlist episode picker -->
  {#if playlistRequest}
    {@const visibleEntries = playlistRequest.entries ?? []}
    {@const allIndices = visibleEntries.map((entry: any) => entry.index).filter((index: any) => typeof index === 'number')}
    {@const selectedCount = playlistSelection.length}
    {@const isAllSelected = allIndices.length > 0 && selectedCount === allIndices.length}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="settings-backdrop" transition:fade={{ duration: 160 }} onclick={dismissPlaylist}>
      <div
        class="settings-panel glass playlist-panel"
        role="dialog"
        aria-modal="true"
        aria-label={t('playlist.title')}
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
      >
        <div class="settings-header">
          <h3>{t('playlist.title')}</h3>
          <button class="close-settings" onclick={dismissPlaylist} title={t('playlist.cancel')} aria-label={t('playlist.cancel')}>
            <IconX size={18} />
          </button>
        </div>

        <div class="playlist-meta">
          <span class="playlist-title">{playlistRequest.title}</span>
          <span class="playlist-count">
            {t('playlist.subtitle', { count: playlistRequest.count ?? visibleEntries.length })}
            {#if playlistRequest.truncated}
              · {t('playlist.more', { shown: visibleEntries.length, count: playlistRequest.count })}
            {/if}
          </span>
        </div>

        <div class="playlist-toolbar">
          <label class="playlist-check">
            <input type="checkbox" checked={isAllSelected} onchange={(e) => toggleAllPlaylistEntries(e.currentTarget.checked)} />
            <span>{t('playlist.all', { count: visibleEntries.length })}</span>
          </label>
          <span class="playlist-selected">{t('playlist.selected', { selected: selectedCount, total: allIndices.length })}</span>
        </div>

        <div class="playlist-list">
          {#each visibleEntries as entry (entry.index)}
            <label class="playlist-entry" class:selected={playlistSelection.includes(entry.index)}>
              <input
                type="checkbox"
                checked={playlistSelection.includes(entry.index)}
                onchange={(e) => togglePlaylistEntry(entry.index, e.currentTarget.checked)}
              />
              <span class="playlist-entry-index">{entry.index}</span>
              <span class="playlist-entry-title" title={entry.title}>{entry.title || t('playlist.episode', { index: entry.index })}</span>
              {#if entry.duration}
                <span class="playlist-entry-duration">{entry.duration}</span>
              {/if}
            </label>
          {/each}
        </div>

        {#if playlistError}
          <p class="sniffer-error">{playlistError}</p>
        {/if}

        <div class="playlist-actions">
          <button class="toast-btn secondary" onclick={dismissPlaylist} disabled={playlistBusy}>{t('playlist.cancel')}</button>
          <button class="toast-btn secondary" onclick={() => confirmPlaylistSelection(true)} disabled={playlistBusy}>
            {t('playlist.download_all', { count: playlistRequest.count ?? visibleEntries.length })}
          </button>
          <button class="toast-btn primary" onclick={() => confirmPlaylistSelection()} disabled={playlistBusy || selectedCount === 0}>
            {playlistBusy ? t('download.submitting') : t('playlist.confirm', { selected: selectedCount })}
          </button>
        </div>
      </div>
    </div>
  {/if}
</main>

<style>
  .app-container {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative;
    box-sizing: border-box;
    min-height: 0;
    overflow: hidden;
  }

  .workspace {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    scrollbar-gutter: stable;
    overscroll-behavior: contain;
  }

  /* Drop overlay */
  .drop-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(50, 46, 40, 0.26);
    backdrop-filter: blur(2px);
    z-index: 1000;
    display: flex;
    justify-content: center;
    align-items: center;
  }

  .drop-card {
    background: var(--bg-card);
    border: 1.5px dashed var(--border-focus);
    border-radius: var(--radius-xl);
    padding: 36px;
    text-align: center;
    width: 80%;
    max-width: 400px;
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .drop-card h2 {
    font-family: var(--font-display);
    margin-top: 20px;
    margin-bottom: 8px;
    font-weight: 600;
    text-wrap: balance;
  }

  .drop-card p {
    color: var(--text-secondary);
    margin: 0;
    text-wrap: pretty;
  }

  /* Window Header */
  .window-header {
    flex: 0 0 auto;
    height: 60px;
    padding: 0 var(--page-gutter);
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border-color);
  }

  /* Transparent native titlebars already reserve a strip for window controls. */
  :global(html.native-mac) .window-header {
    height: 52px;
  }

  .header-title {
    font-family: var(--font-display);
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.22em;
    color: var(--text-primary);
  }

  .header-actions {
    margin-left: auto;
  }

  /* MUJI wordmark: plain charcoal, no gradient fill. */
  .gradient-text {
    color: var(--text-primary);
    background: none;
    -webkit-background-clip: initial;
    background-clip: initial;
    -webkit-text-fill-color: currentColor;
  }

  .settings-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    transition-property: background-color, color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .settings-btn:hover {
    background: rgba(47, 45, 41, 0.07);
    color: var(--text-primary);
  }

  .settings-btn:active {
    scale: 0.96;
  }

  /* URL Paste Section */
  .paste-section {
    flex: 0 0 auto;
    padding: 16px var(--page-gutter) 0;
  }

  .input-glow-wrapper {
    display: flex;
    align-items: center;
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-xl);
    padding: 4px 4px 4px 16px;
    transition-property: border-color, box-shadow, background-color;
    transition-duration: 160ms;
    transition-timing-function: ease-out;
    box-shadow: none;
  }

  .input-glow-wrapper:focus-within {
    border-color: var(--border-focus);
    background: var(--bg-input);
    box-shadow: 0 0 0 3px rgba(47, 45, 41, 0.08);
  }

  .url-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 14px;
    height: 42px;
    min-width: 0;
  }

  .url-input::placeholder {
    color: var(--text-muted);
  }

  .url-input:focus-visible {
    outline: none;
  }

  .download-trigger-btn {
    flex: 0 0 auto;
    white-space: nowrap;
    background: var(--accent-primary);
    border: 1px solid var(--accent-primary);
    color: #f7f6f2;
    padding: 0 16px;
    height: 42px;
    border-radius: var(--radius-lg);
    font-weight: 500;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    transition-property: background-color, border-color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
    box-shadow: none;
  }

  .download-trigger-btn:hover:not(:disabled) {
    background: var(--accent-secondary);
    border-color: var(--accent-secondary);
  }

  .download-trigger-btn:active:not(:disabled) {
    scale: 0.98;
  }

  .download-trigger-btn:disabled {
    background: var(--bg-track);
    border-color: var(--bg-track);
    color: var(--text-muted);
    box-shadow: none;
    cursor: not-allowed;
  }

  .manual-extract-btn {
    flex: 0 0 auto;
    white-space: nowrap;
    background: transparent;
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    padding: 0 12px;
    height: 42px;
    margin-right: 8px;
    border-radius: var(--radius-lg);
    font-weight: 500;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    transition-property: background-color, border-color, color;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .manual-extract-btn:hover:not(:disabled) {
    background: var(--bg-track);
    border-color: var(--border-focus);
    color: var(--text-primary);
  }

  .manual-extract-btn:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .app-banner {
    flex: 0 0 auto;
    margin: 12px var(--page-gutter) 0;
    padding: 9px 12px;
    border: 1px solid #e6c8c2;
    border-radius: var(--radius-md);
    background: #f8ecea;
    color: var(--danger-text);
    font-size: 12px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  /* Tabs Nav */
  .tabs-nav {
    flex: 0 0 auto;
    flex-wrap: wrap;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--page-gutter);
    margin: 24px 0 12px;
    gap: 8px 12px;
  }

  .tabs-list {
    min-width: 0;
    flex-wrap: wrap;
    display: flex;
    gap: 4px;
    background: var(--bg-track);
    padding: 3px;
    border-radius: var(--radius-lg);
    box-shadow: none;
  }

  .tab-btn {
    white-space: nowrap;
    flex: 1 0 auto;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    min-height: 38px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 500;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition-property: background-color, color, scale, box-shadow;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tab-btn.active {
    background: var(--bg-card);
    color: var(--text-primary);
    box-shadow: var(--shadow-sm);
  }

  .tab-btn:active {
    scale: 0.98;
  }

  .tab-btn span {
    background: #e3e0d9;
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: 10px;
    color: var(--text-secondary);
  }

  .tab-count {
    min-width: 1ch;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .tab-btn.active span {
    background: var(--accent-primary);
    color: #f7f6f2;
  }

  .clear-btn {
    margin-left: auto;
    flex: 0 0 auto;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    cursor: pointer;
    min-height: 40px;
    padding: 0 4px;
    transition-property: color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .clear-btn:hover {
    color: var(--text-secondary);
  }

  .clear-btn:active {
    scale: 0.96;
  }

  /* Downloads Area */
  .downloads-area {
    flex: 1 0 auto;
    display: flex;
    flex-direction: column;
    padding: 0 var(--page-gutter) var(--page-gutter);
  }

  .capture-mode .downloads-area { flex-grow: 0; }
  .capture-mode .empty-state { min-height: 160px; }

  .tasks-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  /* Task Card */
  .task-card {
    border-radius: var(--radius-xl);
    display: flex;
    align-items: center;
    overflow: hidden;
    min-height: 76px;
    padding: 10px 12px;
    gap: 12px;
    box-sizing: border-box;
  }

  .service-icon {
    width: 44px;
    height: 44px;
    flex: 0 0 44px;
    border-radius: var(--radius-md);
    color: var(--service-color);
    background: var(--service-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: inset 0 0 0 1px rgba(47, 45, 41, 0.06);
  }

  .task-info-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    overflow: hidden;
  }

  .task-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }

  .task-title-group {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .task-title {
    font-size: 13.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .task-service {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0;
    opacity: 0.85;
  }

  .task-status-badge {
    flex: 0 0 auto;
    font-size: 11px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }

  .task-status-badge.queued { background: var(--bg-track); color: var(--text-secondary); }
  .task-status-badge.analyzing { background: var(--warning-gradient); color: #fff; }
  .task-status-badge.downloading { background: var(--accent-primary); color: #f7f6f2; }
  .task-status-badge.merging { background: #6a655d; color: #f7f6f2; }
  .task-status-badge.completed { background: var(--success-gradient); color: #fff; }
  .task-status-badge.failed { background: var(--danger-gradient); color: #fff; }
  .task-status-badge.cancelled { background: var(--bg-track); color: var(--text-muted); }

  /* Progress bar styles */
  .progress-bar-wrapper {
    margin: 7px 0;
  }

  .progress-bar-bg {
    height: 5px;
    background: #e8e5de;
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.15s linear;
    background: var(--accent-primary);
  }

  .progress-bar-fill.analyzing {
    background: var(--warning-gradient);
    animation: pulse 1.5s infinite alternate;
  }

  .progress-bar-fill.indeterminate {
    background: var(--accent-primary);
    animation: pulse 1.5s infinite alternate;
  }

  .progress-bar-fill.merging {
    background: repeating-linear-gradient(45deg, #2f2d29 0 6px, #6a655d 6px 12px);
    background-size: 200% 100%;
    animation: moveGrad 2s linear infinite;
  }

  .progress-bar-fill.completed {
    background: var(--success-gradient);
  }

  .progress-bar-fill.failed {
    background: var(--danger-gradient);
  }

  .progress-bar-fill.cancelled {
    background: #d6d2c9;
  }

  @keyframes pulse {
    0% { opacity: 0.6; width: 20%; }
    100% { opacity: 1; width: 60%; }
  }

  @keyframes moveGrad {
    0% { background-position: 0% 50%; }
    100% { background-position: 200% 50%; }
  }

  .task-status-footer {
    flex-wrap: wrap;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 4px 12px;
    min-width: 0;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .stats-text {
    display: inline-block;
    min-width: 0;
    font-variant-numeric: tabular-nums;
  }

  .stats-text.speed {
    color: var(--text-primary);
    font-weight: 500;
  }

  .stats-text.success { color: var(--success-text); }
  .stats-text.error { color: var(--danger-text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 420px; }
  .stats-text.warning { color: var(--warning-text); }

  .animated-dots::after {
    content: '...';
    display: inline-block;
    width: 12px;
    animation: dots 1.5s infinite steps(4);
    text-align: left;
  }

  @keyframes dots {
    0% { content: ''; }
    25% { content: '.'; }
    50% { content: '..'; }
    75% { content: '...'; }
  }

  /* Actions column */
  .task-actions-col {
    flex: 0 0 auto;
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .action-circle-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: 1px solid var(--border-color);
    background: var(--bg-subtle);
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: none;
    transition-property: background-color, color, border-color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .action-circle-btn:hover {
    background: #e9e6df;
    color: var(--text-primary);
    border-color: var(--border-hover);
    box-shadow: none;
  }

  .action-circle-btn.danger:hover {
    background: #f7e9e6;
    color: var(--danger-text);
    border-color: #e3c2bc;
  }

  .action-circle-btn.success:hover {
    background: #e9f0e6;
    color: var(--success-text);
    border-color: #c8d5c2;
  }

  .action-circle-btn:active {
    scale: 0.96;
  }

  /* Empty State */
  .empty-state {
    flex: 1;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 38px 32px 34px;
    min-height: 240px;
  }

  .empty-icon-pulse {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background: var(--bg-subtle);
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 22px;
    box-shadow: inset 0 0 0 1px var(--border-color);
  }

  .empty-state h3 {
    font-family: var(--font-display);
    font-size: 18px;
    margin: 0 0 8px 0;
    font-weight: 600;
    text-wrap: balance;
  }

  .empty-state p {
    color: var(--text-secondary);
    font-size: 13px;
    width: min(100%, 520px);
    text-wrap: pretty;
    margin: 0;
    line-height: 1.5;
  }

  /* Clipboard Toast Stack */
  /* The stack is the positioned element so several toasts stack upward instead of
     overlapping each other in the same corner slot. */
  .toast-stack {
    position: absolute;
    bottom: 20px;
    left: 20px;
    right: 20px;
    z-index: 500;
    display: flex;
    flex-direction: column-reverse;
    gap: 10px;
    pointer-events: none;
  }

  .toast-stack > * { pointer-events: auto; }

  .clipboard-toast {
    border-radius: var(--radius-xl);
    padding: 14px 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    box-shadow: var(--shadow-lg);
  }

  /* Available to assistive tech, invisible on screen. */
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }

  .toast-content {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1;
    overflow: hidden;
  }

  .toast-text {
    overflow: hidden;
  }

  .toast-text h4 {
    margin: 0 0 2px 0;
    font-size: 12.5px;
    font-weight: 600;
    text-wrap: balance;
  }

  .toast-text p {
    margin: 0;
    font-size: 11px;
    color: var(--text-secondary);
    text-wrap: pretty;
  }

  .truncate {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .toast-actions {
    display: flex;
    gap: 8px;
  }

  .toast-btn {
    border: none;
    font-size: 11px;
    font-weight: 600;
    min-height: 40px;
    padding: 0 12px;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition-property: background-color, color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .toast-btn.primary {
    background: var(--accent-primary);
    color: #f7f6f2;
  }

  .toast-btn.secondary {
    background: var(--bg-track);
    color: var(--text-primary);
  }

  .toast-btn.secondary:hover {
    background: #e3e0d9;
  }

  .toast-btn:active {
    scale: 0.96;
  }

  /* Settings Panel Overlay */
  .settings-backdrop {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(50, 46, 40, 0.26);
    backdrop-filter: blur(2px);
    z-index: 900;
    display: flex;
    justify-content: flex-end;
  }

  .settings-panel {
    box-sizing: border-box;
    min-height: 0;
    width: min(360px, 100%);
    height: 100%;
    background: #fbfaf7;
    border-left: 1px solid var(--border-color);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
  }

  /* A docked panel is not an interactive surface; don't let the shared .glass
     hover treatment react as the pointer moves across it. */
  .settings-panel:hover {
    background: #fbfaf7;
    border-color: var(--border-color);
    box-shadow: var(--shadow-lg);
  }

  .settings-header {
    flex: 0 0 auto;
    box-sizing: border-box;
    height: 52px;
    padding: 0 20px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border-color);
  }

  .settings-header h3 {
    font-family: var(--font-display);
    font-size: 15px;
    margin: 0;
    font-weight: 600;
    text-wrap: balance;
  }

  .close-settings {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    transition-property: background-color, color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .close-settings:hover {
    background: rgba(47, 45, 41, 0.07);
    color: var(--text-primary);
  }

  .close-settings:active {
    scale: 0.96;
  }

  .settings-body {
    flex: 1;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 20px;
    overflow-y: auto;
    min-height: 0;
    overflow-x: hidden;
  }

  .setting-item {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .setting-item label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-secondary);
    letter-spacing: 0;
  }

  .path-selector {
    display: flex;
    gap: 6px;
  }

  .path-input {
    flex: 1;
    min-width: 0;
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 40px;
    outline: none;
    text-overflow: ellipsis;
  }

  .btn-select-dir {
    flex: 0 0 40px;
    background: var(--bg-subtle);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    width: 40px;
    height: 40px;
    border-radius: var(--radius-md);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: none;
    transition-property: background-color, border-color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .btn-select-dir:hover {
    background: #e9e6df;
    border-color: var(--border-hover);
    box-shadow: none;
  }

  .btn-select-dir:active {
    scale: 0.96;
  }

  .settings-select {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 40px;
    outline: none;
  }

  .checkbox-item {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    min-height: 40px;
    margin-top: 6px;
  }

  .checkbox-item input {
    margin: 0;
    cursor: pointer;
    width: 17px;
    height: 17px;
    accent-color: var(--text-primary);
  }

  .checkbox-item label {
    font-size: 12px;
    font-weight: 500;
    text-transform: none;
    color: var(--text-primary);
    letter-spacing: 0;
    cursor: pointer;
  }

  .settings-footer {
    padding: 20px;
    border-top: 1px solid var(--border-color);
  }

  .setting-divider {
    height: 1px;
    background: var(--border-color);
    margin: 4px 0;
  }

  .setting-section-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-primary);
    margin-top: 4px;
  }

  .settings-input {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 40px;
    outline: none;
    width: 100%;
    box-sizing: border-box;
    font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
  }

  .settings-input:focus {
    border-color: var(--border-focus);
  }

  .settings-input::placeholder {
    color: var(--text-muted);
    font-family: var(--font-display);
  }

  .setting-hint {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.45;
    text-wrap: pretty;
  }

  .settings-app-version {
    margin: 0;
    font-size: 11px;
    color: var(--text-muted);
    text-align: center;
  }

  /* Download engine row */
  .engine-item {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .engine-status {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .engine-version {
    font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
    font-size: 12px;
    color: var(--text-primary);
  }

  .engine-source {
    padding: 2px 7px;
    border-radius: var(--radius-sm);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .engine-source.bundled {
    color: var(--text-secondary);
    background: var(--bg-subtle);
  }

  .engine-source.updated {
    color: var(--success-text);
    background: #eaf0e6;
  }

  .engine-source.missing {
    color: var(--danger-text);
    background: #f7e6e6;
  }

  .engine-actions {
    display: flex;
    gap: 6px;
    flex: 0 0 auto;
  }

  /* Playlist episode picker */
  .playlist-panel {
    width: min(560px, 92vw);
    max-height: min(680px, 88vh);
    display: flex;
    flex-direction: column;
  }

  .playlist-meta {
    display: grid;
    gap: 4px;
    padding: 0 20px;
  }

  .playlist-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .playlist-count {
    font-size: 11px;
    color: var(--text-muted);
  }

  .playlist-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin: 0 20px;
    padding: 9px 11px;
    border: 1px solid var(--border-color);
    border-radius: var(--radius-md);
    background: var(--bg-subtle);
  }

  .playlist-check,
  .playlist-entry {
    display: flex;
    align-items: center;
    gap: 9px;
    cursor: pointer;
  }

  .playlist-check {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .playlist-selected {
    font-size: 11px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .playlist-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 20px;
  }

  .playlist-entry {
    padding: 8px 10px;
    border-radius: var(--radius-md);
    border: 1px solid transparent;
    color: var(--text-secondary);
    font-size: 12px;
    transition: background-color .15s ease, border-color .15s ease;
  }

  .playlist-entry:hover {
    background: var(--bg-subtle);
  }

  .playlist-entry.selected {
    border-color: #dedbd3;
    background: var(--bg-card);
    color: var(--text-primary);
  }

  .playlist-entry-index {
    flex: 0 0 auto;
    min-width: 22px;
    font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
    font-size: 11px;
    color: var(--text-muted);
    text-align: right;
  }

  .playlist-entry-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-entry-duration {
    flex: 0 0 auto;
    font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
    font-size: 11px;
    color: var(--text-muted);
  }

  .playlist-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 16px 20px 20px;
    border-top: 1px solid var(--border-color);
  }

  .mode-switch {
    flex: 0 0 auto;
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin: var(--section-gap) var(--page-gutter) 0;
    padding: 3px;
    background: var(--bg-track);
    border: 1px solid transparent;
    border-radius: var(--radius-lg);
  }

  .mode-switch button, .sniffer-control, .capture-download, .capture-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 0;
    font: inherit;
    cursor: pointer;
  }
  .mode-switch button { color: var(--text-secondary); background: transparent; padding: 7px 12px; border-radius: var(--radius-md); font-size: 12px; transition: background-color .15s ease, color .15s ease; }
  .mode-switch button.active { color: #f7f6f2; background: var(--accent-primary); }
  /* Capture setup and downloads share the workspace scroll, so neither is squeezed shut. */
  .sniffer-section {
    flex: 0 0 auto;
    margin: var(--section-gap) var(--page-gutter) 0;
    padding: 16px;
    border: 1px solid var(--border-color);
    border-radius: var(--radius-xl);
    background: var(--bg-card);
  }
  .sniffer-heading { display:flex; align-items:flex-start; justify-content:space-between; gap:18px; }
  .sniffer-heading h2 { margin:0; color:var(--text-primary); font-size:18px; font-weight:600; }
  .sniffer-heading p { margin:7px 0 0; color:var(--text-secondary); font-size:13px; line-height:1.55; }
  .sniffer-control { flex:0 0 auto; padding:9px 14px; color:#f7f6f2; background:var(--accent-primary); border-radius:var(--radius-md); font-size:12px; font-weight:500; }
  .sniffer-control.stop { background:var(--danger-gradient); }
  .sniffer-control:disabled, .capture-clear:disabled, .capture-download:disabled { opacity:.45; cursor:not-allowed; }
  .sniffer-notice { display:grid; gap:4px; margin-top:17px; padding:11px 12px; color:var(--text-secondary); border-left:2px solid var(--text-primary); background:var(--bg-subtle); font-size:12px; line-height:1.55; }
  .sniffer-notice strong { color:var(--text-primary); }
  .sniffer-tun-warning { margin:6px 0 2px; padding:8px 10px; color:var(--warning-text); border:1px solid #e8d6b8; border-radius:var(--radius-md); background:#faf3e4; font-size:12px; line-height:1.55; }
  .sniffer-setup-actions { display:flex; flex-wrap:wrap; gap:7px; margin-top:6px; }
  .sniffer-setup-actions button { padding:6px 10px; color:#4a463f; background:var(--bg-subtle); border:1px solid #dedbd3; border-radius:var(--radius-sm); font:inherit; font-size:11px; cursor:pointer; transition:background-color .15s ease, border-color .15s ease, color .15s ease; }
  .sniffer-setup-actions button:hover:not(:disabled) { background:#e9e6df; }
  .sniffer-setup-actions button:disabled { opacity:.45; cursor:not-allowed; }
  .sniffer-setup-actions button.complete { color:var(--success-text); border-color:#c7d4c0; background:#eaf0e6; opacity:1; }
  .sniffer-error { margin:11px 0 0; color:var(--danger-text); font-size:12px; line-height:1.5; }
  .sniffer-message { margin:11px 0 0; color:var(--success-text); font-size:12px; line-height:1.5; }
  .sniffer-capture-header { display:flex; align-items:center; justify-content:space-between; flex-wrap:wrap; gap:10px; margin-top:17px; color:var(--text-secondary); font-size:13px; }
  .sniffer-capture-header b { color:var(--text-primary); font-variant-numeric:tabular-nums; }
  .capture-header-tools { display:flex; flex-wrap:wrap; align-items:center; justify-content:flex-end; gap:8px; margin-left:auto; min-width:0; max-width:100%; }
  .sniffer-hook-status { color:var(--text-muted); font-size:11px; }
  .sniffer-hook-status.ready { color:var(--success-text); }
  .capture-search { width:180px; min-width:0; max-width:100%; box-sizing:border-box; height:30px; padding:0 10px; color:var(--text-primary); background:var(--bg-input); border:1px solid var(--border-color); border-radius:var(--radius-md); font:inherit; font-size:12px; outline:none; transition:border-color .15s ease, box-shadow .15s ease; }
  .capture-search::placeholder { color:var(--text-muted); }
  .capture-search:focus { border-color:var(--border-focus); box-shadow:0 0 0 3px rgba(47,45,41,.1); }
  .capture-clear { width:30px; height:30px; color:var(--text-muted); background:transparent; border-radius:var(--radius-md); transition:color .15s ease, background .15s ease, transform .12s ease; }
  .capture-clear:hover:not(:disabled) { color:var(--text-primary); background:rgba(47,45,41,.07); }
  .capture-clear:active:not(:disabled) { transform:scale(.96); }
  .sniffer-empty { display:flex; flex-direction:column; align-items:center; gap:9px; padding:27px 10px 12px; color:var(--text-muted); text-align:center; font-size:12px; }
  /* The section itself scrolls now, so the grid must not open a second scroll area. */
  .capture-grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(min(100%,228px),1fr)); gap:12px; margin-top:12px; padding:2px 4px 10px; }
  .capture-card { display:flex; flex-direction:column; gap:9px; padding:9px; border-radius:var(--radius-lg); background:var(--bg-card-hover); border:1px solid var(--border-color); box-shadow:none; transition:border-color .15s ease, background .15s ease; }
  .capture-card:hover { background:var(--bg-card); border-color:var(--border-hover); }
  .capture-thumb { position:relative; aspect-ratio:16/9; border-radius:var(--radius-md); overflow:hidden; background:var(--bg-subtle); }
  .capture-thumb img { width:100%; height:100%; object-fit:cover; outline:1px solid rgba(47,45,41,.08); outline-offset:-1px; }
  .capture-thumb-fallback { width:100%; height:100%; display:flex; align-items:center; justify-content:center; color:var(--text-muted); background:var(--bg-subtle); }
  .capture-kind-pill { position:absolute; top:6px; left:6px; padding:2px 7px; border-radius:var(--radius-sm); color:#f7f6f2; background:rgba(43,41,38,.72); font-size:10px; font-weight:600; letter-spacing:.4px; backdrop-filter:blur(2px); }
  .capture-info { min-width:0; display:grid; gap:3px; }
  .capture-info strong { overflow:hidden; color:var(--text-primary); font-size:12.5px; font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .capture-info span { color:var(--text-muted); font-size:11px; }
  .capture-download { display:inline-flex; align-items:center; justify-content:center; gap:6px; min-height:34px; padding:0 10px; color:#f7f6f2; background:var(--accent-primary); border:1px solid var(--accent-primary); border-radius:var(--radius-md); font-size:12px; font-weight:500; cursor:pointer; transition:background .15s ease, border-color .15s ease, transform .12s ease; }
  .capture-download:hover:not(:disabled) { background:var(--accent-secondary); border-color:var(--accent-secondary); }
  .capture-download:active:not(:disabled) { transform:scale(.97); }
  .capture-download:disabled { opacity:.45; cursor:not-allowed; }
  .capture-grid::-webkit-scrollbar { width:8px; }
  .capture-grid::-webkit-scrollbar-thumb { background:rgba(58,53,45,.18); border-radius:4px; }
  .capture-grid::-webkit-scrollbar-thumb:hover { background:rgba(58,53,45,.3); }

  @media (max-width: 600px) {
    .app-container { --page-gutter: 16px; }
    .tabs-list { flex: 1; gap: 2px; }
    .tab-btn { justify-content: center; padding: 0 9px; }
    .sniffer-heading { flex-wrap: wrap; gap: 12px; }
    .empty-state { padding-left: 16px; padding-right: 16px; }
    .task-card { gap: 10px; padding-left: 10px; padding-right: 10px; }
    .service-icon { width: 36px; height: 36px; flex-basis: 36px; }
    .task-actions-col { flex-direction: column; }
  }

  @media (max-width: 400px) {
    .tabs-list { flex-basis: 100%; }
    .tab-btn { flex-basis: calc(50% - 2px); }
    .sniffer-heading { flex-direction: column; }
    .sniffer-control { align-self: flex-start; }
    .capture-header-tools { width: 100%; }
    .capture-search { flex: 1; }
  }
</style>
