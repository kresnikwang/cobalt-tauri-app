<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  
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
    IconCloudDownload
  } from "@tabler/icons-svelte";

  import { t, getLocale, setLocale } from '$lib/i18n.svelte';
  import { platforms, getServiceInfo } from '$lib/services';

  // State declaration using Svelte 5 Runes
  let inputUrl = $state('');
  let isDragging = $state(false);
  let showSettings = $state(false);
  let clipboardToast = $state<{ url: string; visible: boolean }>({ url: '', visible: false });
  let clipboardTimeout: any = null;
  
  // Settings State
  let settings = $state({
    savePath: '',
    apiUrl: 'http://43.156.122.169',
    downloadMode: 'video',
    videoQuality: '720',
    audioFormat: 'best',
    clipboardMonitoring: true,
    maxParallelDownloads: 3,
    proxyEnabled: true,
    proxyUrl: 'http://127.0.0.1:7897'
  });

  // Tasks List
  let tasks = $state<any[]>([]);
  let activeTab = $state<'all' | 'downloading' | 'completed' | 'failed'>('all');

  // Filter tasks based on active tab using Svelte 5 $derived rune
  let filteredTasks = $derived(
    activeTab === 'downloading'
      ? tasks.filter(t => ['downloading', 'analyzing', 'queued', 'merging'].includes(t.status))
      : activeTab === 'completed'
      ? tasks.filter(t => t.status === 'completed')
      : activeTab === 'failed'
      ? tasks.filter(t => ['failed', 'cancelled'].includes(t.status))
      : tasks
  );

  let unlistenTask: (() => void) | null = null;
  let unlistenClipboard: (() => void) | null = null;

  onMount(async () => {
    // Get settings from Tauri backend
    settings = await invoke('get_settings');
    // Get current tasks
    tasks = await invoke('get_tasks');

    // Listen for task updates from Tauri Rust process
    unlistenTask = await listen('task-updated', (event) => {
      const updatedTask = event.payload as any;
      const index = tasks.findIndex(t => t.id === updatedTask.id);
      if (index !== -1) {
        tasks[index] = updatedTask;
        tasks = [...tasks]; // force Svelte 5 array proxy update
      } else {
        tasks = [updatedTask, ...tasks];
      }
    });

    // Listen for clipboard events
    unlistenClipboard = await listen('clipboard-detected', (event) => {
      const url = event.payload as string;
      clipboardToast = { url, visible: true };

      // Auto hide toast after 8 seconds
      if (clipboardTimeout) clearTimeout(clipboardTimeout);
      clipboardTimeout = setTimeout(() => {
        clipboardToast.visible = false;
      }, 8000);
    });
  });

  onDestroy(() => {
    if (unlistenTask) unlistenTask();
    if (unlistenClipboard) unlistenClipboard();
    if (clipboardTimeout) clearTimeout(clipboardTimeout);
  });

  async function handleDownload(urlToDownload = inputUrl) {
    if (!urlToDownload.trim()) return;
    const cleanUrl = urlToDownload.trim();
    inputUrl = '';
    clipboardToast.visible = false;
    if (clipboardTimeout) clearTimeout(clipboardTimeout);
    
    await invoke('download_url', { url: cleanUrl });
  }

  async function selectDirectory() {
    const path: string | null = await invoke('select_directory');
    if (path) {
      settings.savePath = path;
      await saveSettings();
    }
  }

  async function saveSettings() {
    settings = await invoke('save_settings', { newSettings: JSON.parse(JSON.stringify(settings)) });
  }

  async function cancelTask(id: string) {
    await invoke('cancel_task', { id });
  }

  async function deleteTask(id: string) {
    await invoke('delete_task', { id });
    tasks = tasks.filter(t => t.id !== id);
  }

  async function clearCompleted() {
    tasks = await invoke('clear_completed');
  }

  async function revealInFinder(path: string) {
    await invoke('reveal_in_finder', { path });
  }

  async function openFile(path: string) {
    await invoke('open_file', { path });
  }

  // Drag & Drop
  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  // svelte-ignore a11y_no_static_element_interactions
  function handleDragLeave() {
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    const text = e.dataTransfer?.getData('text');
    
    if (text) {
      handleDownload(text);
    }
  }

  function formatBytes(bytes: number, decimals = 2) {
    if (!bytes || bytes === 0) return '0 Bytes';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
  }

  function savePathLabel(path: string) {
    const parts = path?.split(/[\\/]/).filter(Boolean) ?? [];
    return parts.at(-1) || 'Downloads';
  }

  function qualityLabel() {
    if (settings.downloadMode === 'audio') {
      return settings.audioFormat === 'best' ? t('settings.audio.best') : settings.audioFormat.toUpperCase();
    }
    return settings.videoQuality === 'max' ? t('settings.quality.max') : `${settings.videoQuality}p`;
  }

  function platformCapability(name: string) {
    const key = name.toLowerCase();
    if (key.includes('youtube')) return t('platform.capability.local_first');
    if (key.includes('bilibili')) return t('platform.capability.cookie_hd');
    if (key.includes('dailymotion')) return t('platform.capability.local_first');
    if (key.includes('soundcloud')) return t('platform.capability.audio');
    if (key.includes('instagram') || key.includes('twitter') || key.includes('x') || key.includes('pinterest')) {
      return t('platform.capability.server_assist');
    }
    return t('platform.capability.basic');
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
  <header class="window-header drag-handle">
    <div class="header-title no-drag">
      <span class="gradient-text">COBALT</span>
    </div>
    <div class="header-actions no-drag">
      <button class="settings-btn" onclick={() => showSettings = !showSettings} title={t('settings.title')} aria-label={t('settings.title')}>
        <IconSettings size={18} />
      </button>
    </div>
  </header>

  <!-- URL Paste Section -->
  <section class="paste-section">
    <div class="input-glow-wrapper">
      <input 
        type="text" 
        placeholder={t('input.placeholder')}
        bind:value={inputUrl}
        onkeydown={(e) => e.key === 'Enter' && handleDownload()}
        autocapitalize="off"
        autocomplete="off"
        spellcheck="false"
        class="url-input"
      />
      <button class="download-trigger-btn" onclick={() => handleDownload()} disabled={!inputUrl.trim()}>
        <IconDownload size={18} />
        <span>{t('analyze')}</span>
      </button>
    </div>
    <div class="download-context-strip">
      <span class="context-pill">{settings.downloadMode === 'audio' ? t('settings.audio_only') : t('settings.video_audio')}</span>
      <span class="context-pill">{qualityLabel()}</span>
      <span class="context-pill">{t('home.save_to')}: {savePathLabel(settings.savePath)}</span>
      <span class="context-pill highlight">{t('home.youtube_local')}</span>
    </div>
  </section>

  <!-- Tabs Navigation -->
  <nav class="tabs-nav">
    <div class="tabs-list">
      <button class="tab-btn" class:active={activeTab === 'all'} onclick={() => activeTab = 'all'}>
        {t('tabs.all')} <span class="tab-count">{tasks.length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'downloading'} onclick={() => activeTab = 'downloading'}>
        {t('tabs.downloading')} <span class="tab-count">{tasks.filter(t => ['downloading', 'analyzing', 'merging'].includes(t.status)).length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'completed'} onclick={() => activeTab = 'completed'}>
        {t('tabs.completed')} <span class="tab-count">{tasks.filter(t => t.status === 'completed').length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'failed'} onclick={() => activeTab = 'failed'}>
        {t('tabs.failed')} <span class="tab-count">{tasks.filter(t => ['failed', 'cancelled'].includes(t.status)).length}</span>
      </button>
    </div>
    {#if tasks.some(t => ['completed', 'failed', 'cancelled'].includes(t.status))}
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
          <IconCloudDownload size={40} color="var(--text-muted)" />
        </div>
        <h3>{t('empty.title')}</h3>
        <p>{t('empty.subtitle')}</p>
        
        <span class="platforms-title">{t('empty.sources')}</span>
        <div class="platforms-grid">
          {#each platforms as platform}
            <div class="platform-card">
              <span class="platform-card-name" style="color: {platform.color}">{platform.name}</span>
              <span class="platform-card-domain">{platform.domain}</span>
              <span class="platform-card-capability">{platformCapability(platform.name)}</span>
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <div class="tasks-list">
        {#each filteredTasks as task (task.id)}
          {@const service = getServiceInfo(task.url, t('service.unknown'))}
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
                <div class="progress-bar-bg">
                  <div 
                    class="progress-bar-fill {task.status}" 
                    class:indeterminate={task.status === 'downloading' && task.totalBytes === 0}
                    style="width: {task.progress * 100}%"
                  ></div>
                </div>
              </div>

              <!-- Status line -->
              <div class="task-status-footer">
                {#if task.status === 'downloading'}
                  <span class="stats-text">
                    {formatBytes(task.downloadedBytes)} / {task.totalBytes > 0 ? formatBytes(task.totalBytes) : t('task.unknown_size')}
                  </span>
                  <span class="stats-text speed">{task.speed}</span>
                  <span class="stats-text eta">{t('task.eta')}: {task.eta}</span>
                {:else if task.status === 'analyzing'}
                  <span class="stats-text animated-dots">{t('task.connecting')}</span>
                {:else if task.status === 'merging'}
                  <span class="stats-text animated-dots font-semibold text-indigo-400">{t('task.merging')}</span>
                {:else if task.status === 'completed'}
                  <span class="stats-text success">{t('task.completed')}</span>
                {:else if task.status === 'failed'}
                  <span class="stats-text error" title={task.error}>{humanTaskError(task.error)}</span>
                {:else if task.status === 'cancelled'}
                  <span class="stats-text warning">{t('task.cancelled')}</span>
                {/if}
              </div>
            </div>

            <!-- Actions Column -->
            <div class="task-actions-col">
              {#if ['downloading', 'analyzing', 'queued'].includes(task.status)}
                <button class="action-circle-btn danger" onclick={() => cancelTask(task.id)} title="Cancel">
                  <IconX size={14} />
                </button>
              {:else if task.status === 'completed'}
                <button class="action-circle-btn success" onclick={() => openFile(task.outputPath)} title="Play File">
                  <IconPlayerPlay size={14} />
                </button>
                <button class="action-circle-btn secondary" onclick={() => revealInFinder(task.outputPath)} title="Show in Finder">
                  <IconFolder size={14} />
                </button>
                <button class="action-circle-btn secondary" onclick={() => deleteTask(task.id)} title="Remove from List">
                  <IconTrash size={14} />
                </button>
              {:else}
                <button class="action-circle-btn" onclick={() => handleDownload(task.url)} title="Retry">
                  <IconPlayerPlay size={14} />
                </button>
                <button class="action-circle-btn secondary" onclick={() => deleteTask(task.id)} title="Remove from List">
                  <IconTrash size={14} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <!-- Clipboard Toast Slide-up -->
  {#if clipboardToast.visible}
    <div class="clipboard-toast glass" in:fly={{ y: 12, duration: 220 }} out:fly={{ y: -8, duration: 140 }}>
      <div class="toast-content">
        <IconClipboard size={20} color="var(--accent-primary)" />
        <div class="toast-text">
          <h4>{t('toast.detected')}</h4>
          <p class="truncate">{clipboardToast.url}</p>
        </div>
      </div>
      <div class="toast-actions">
        <button class="toast-btn secondary" onclick={() => clipboardToast.visible = false}>{t('toast.ignore')}</button>
        <button class="toast-btn primary" onclick={() => handleDownload(clipboardToast.url)}>{t('toast.download')}</button>
      </div>
    </div>
  {/if}

  <!-- Settings Slide Panel -->
  {#if showSettings}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="settings-backdrop" onclick={() => showSettings = false} transition:fade={{ duration: 160 }}>
      <div class="settings-panel glass" role="dialog" aria-modal="true" aria-label={t('settings.title')} tabindex="-1" onclick={(e) => e.stopPropagation()} in:fly={{ x: 24, duration: 220 }} out:fly={{ x: 16, duration: 140 }}>
        <div class="settings-header">
          <h3>{t('settings.title')}</h3>
          <button class="close-settings" onclick={() => showSettings = false} title={t('settings.title')} aria-label={t('settings.title')}>
            <IconX size={18} />
          </button>
        </div>

        <div class="settings-body">
          <!-- Language -->
          <div class="setting-item">
            <label for="lang-select">{t('settings.language')}</label>
            <select id="lang-select" value={getLocale()} onchange={(e) => setLocale(e.currentTarget.value)} class="settings-select">
              <option value="en">English</option>
              <option value="zh">中文</option>
              <option value="ru">Русский</option>
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
          <p class="settings-app-version">{t('settings.version', { version: '2.0.0' })}</p>
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
  }

  /* Drop overlay */
  .drop-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(8px);
    z-index: 1000;
    display: flex;
    justify-content: center;
    align-items: center;
  }

  .drop-card {
    background: var(--bg-card);
    border: 2px dashed var(--accent-primary);
    border-radius: 18px;
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
    min-height: 56px;
    padding-top: 12px; /* Margin for Traffic Lights on macOS */
    padding-left: 80px; /* Leave space for macOS Traffic Lights */
    padding-right: 20px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border-color);
  }

  .header-title {
    font-family: var(--font-display);
    font-size: 15px;
    font-weight: 700;
    letter-spacing: 0;
    color: var(--text-primary);
  }

  .header-actions {
    margin-left: auto;
  }

  .gradient-text {
    background: var(--accent-gradient);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
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
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }

  .settings-btn:active {
    scale: 0.96;
  }

  /* URL Paste Section */
  .paste-section {
    padding: 20px 20px 14px;
  }

  .input-glow-wrapper {
    display: flex;
    align-items: center;
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 16px;
    padding: 4px 4px 4px 16px;
    transition-property: border-color, box-shadow, background-color;
    transition-duration: 180ms;
    transition-timing-function: ease-out;
    box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.34), 0 0 0 1px rgba(255, 255, 255, 0.015);
  }

  .input-glow-wrapper:focus-within {
    border-color: var(--border-focus);
    background: rgba(9, 10, 14, 0.96);
    box-shadow: 0 0 0 3px rgba(119, 133, 255, 0.12), inset 0 1px 2px rgba(0, 0, 0, 0.36);
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
    background: var(--accent-gradient);
    border: none;
    color: white;
    padding: 0 14px 0 16px;
    height: 42px;
    border-radius: 12px;
    font-weight: 600;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    transition-property: transform, scale, box-shadow, background-color, color;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
    box-shadow: 0 5px 14px rgba(79, 91, 205, 0.28);
  }

  .download-trigger-btn:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 7px 18px rgba(79, 91, 205, 0.36);
  }

  .download-trigger-btn:active:not(:disabled) {
    transform: translateY(0);
    scale: 0.96;
  }

  .download-trigger-btn:disabled {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-muted);
    box-shadow: none;
    cursor: not-allowed;
  }

  .download-context-strip {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
    padding: 0 2px;
  }

  .context-pill {
    height: 22px;
    padding: 0 9px;
    border-radius: 999px;
    display: inline-flex;
    align-items: center;
    max-width: 220px;
    color: var(--text-secondary);
    background: rgba(255, 255, 255, 0.045);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.055);
    font-size: 10.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .context-pill.highlight {
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.1);
    box-shadow: 0 0 0 1px rgba(119, 133, 255, 0.2);
  }

  /* Tabs Nav */
  .tabs-nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 20px;
    margin-bottom: 14px;
    gap: 12px;
  }

  .tabs-list {
    display: flex;
    gap: 8px;
    background: rgba(0, 0, 0, 0.2);
    padding: 3px;
    border-radius: 13px;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.045);
    overflow-x: auto;
  }

  .tab-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    min-height: 40px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 500;
    border-radius: 10px;
    cursor: pointer;
    transition-property: background-color, color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tab-btn.active {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }

  .tab-btn:active {
    scale: 0.96;
  }

  .tab-btn span {
    background: rgba(255, 255, 255, 0.08);
    padding: 1px 6px;
    border-radius: 10px;
    font-size: 10px;
    color: var(--text-secondary);
  }

  .tab-count {
    min-width: 1ch;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .tab-btn.active span {
    background: var(--accent-gradient);
    color: white;
  }

  .clear-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
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
    flex: 1;
    overflow-y: auto;
    padding: 0 20px 20px 20px;
  }

  .tasks-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  /* Task Card */
  .task-card {
    border-radius: 16px;
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
    border-radius: 10px;
    color: var(--service-color);
    background: var(--service-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.065);
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
    text-wrap: pretty;
  }

  .task-service {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0;
    opacity: 0.85;
  }

  .task-status-badge {
    flex: 0 0 auto;
    font-size: 8px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 6px;
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }

  .task-status-badge.queued { background: rgba(255,255,255,0.08); color: var(--text-secondary); }
  .task-status-badge.analyzing { background: var(--warning-gradient); color: white; }
  .task-status-badge.downloading { background: var(--accent-gradient); color: white; }
  .task-status-badge.merging { background: linear-gradient(135deg, #a855f7 0%, #6366f1 100%); color: white; }
  .task-status-badge.completed { background: var(--success-gradient); color: white; }
  .task-status-badge.failed { background: var(--danger-gradient); color: white; }
  .task-status-badge.cancelled { background: rgba(255,255,255,0.08); color: var(--text-muted); }

  /* Progress bar styles */
  .progress-bar-wrapper {
    margin: 7px 0;
  }

  .progress-bar-bg {
    height: 6px;
    background: rgba(0, 0, 0, 0.3);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.15s linear;
    background: var(--accent-gradient);
  }

  .progress-bar-fill.analyzing {
    background: var(--warning-gradient);
    animation: pulse 1.5s infinite alternate;
  }

  .progress-bar-fill.indeterminate {
    background: var(--accent-gradient);
    animation: pulse 1.5s infinite alternate;
  }

  .progress-bar-fill.merging {
    background: linear-gradient(90deg, #a855f7, #6366f1, #a855f7);
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
    background: rgba(255,255,255,0.15);
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
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 12px;
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

  .stats-text.success { color: #10b981; }
  .stats-text.error { color: #ef4444; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 420px; }
  .stats-text.warning { color: var(--text-muted); }

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
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: 0;
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: var(--shadow-border);
    transition-property: background-color, color, box-shadow, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .action-circle-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
    box-shadow: var(--shadow-border-hover);
  }

  .action-circle-btn.danger:hover {
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
    box-shadow: 0 0 0 1px rgba(239, 68, 68, 0.32);
  }

  .action-circle-btn.success:hover {
    background: rgba(16, 185, 129, 0.15);
    color: #10b981;
    box-shadow: 0 0 0 1px rgba(16, 185, 129, 0.32);
  }

  .action-circle-btn:active {
    scale: 0.96;
  }

  /* Empty State */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 38px 32px 34px;
    min-height: 360px;
  }

  .empty-icon-pulse {
    width: 80px;
    height: 80px;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.02);
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 24px;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.055), 0 12px 32px rgba(0, 0, 0, 0.2);
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
    margin: 0 0 32px 0;
    line-height: 1.5;
  }

  /* Clipboard Toast */
  .clipboard-toast {
    position: absolute;
    bottom: 20px;
    left: 20px;
    right: 20px;
    border-radius: 16px;
    padding: 14px 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    z-index: 500;
    background: rgba(22, 22, 33, 0.95);
    box-shadow: 0 0 0 1px rgba(119, 133, 255, 0.28), var(--shadow-lg);
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
    border-radius: 10px;
    cursor: pointer;
    transition-property: background-color, color, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .toast-btn.primary {
    background: var(--accent-gradient);
    color: white;
  }

  .toast-btn.secondary {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }

  .toast-btn.secondary:hover {
    background: rgba(255, 255, 255, 0.12);
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
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(4px);
    z-index: 900;
    display: flex;
    justify-content: flex-end;
  }

  .settings-panel {
    width: min(360px, 100%);
    height: 100%;
    background: rgba(20, 20, 30, 0.85);
    box-shadow: -1px 0 0 rgba(255, 255, 255, 0.07), var(--shadow-lg);
    display: flex;
    flex-direction: column;
  }

  .settings-header {
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
    background: rgba(255, 255, 255, 0.08);
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
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 40px;
    outline: none;
    text-overflow: ellipsis;
  }

  .btn-select-dir {
    background: rgba(255, 255, 255, 0.08);
    border: 0;
    color: var(--text-primary);
    width: 40px;
    height: 40px;
    border-radius: 10px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--shadow-border);
    transition-property: background-color, box-shadow, scale;
    transition-duration: 150ms;
    transition-timing-function: ease-out;
  }

  .btn-select-dir:hover {
    background: rgba(255, 255, 255, 0.12);
    box-shadow: var(--shadow-border-hover);
  }

  .btn-select-dir:active {
    scale: 0.96;
  }

  .settings-select {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 10px;
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
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
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
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0;
    color: var(--accent-primary);
    margin-top: 4px;
  }

  .settings-input {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 10px;
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
    border-color: var(--accent-primary);
  }

  .settings-input::placeholder {
    color: var(--text-muted);
    font-family: var(--font-display);
  }

  .setting-hint {
    font-size: 10px;
    color: var(--text-muted);
    line-height: 1.45;
    text-wrap: pretty;
  }

  .settings-app-version {
    margin: 0;
    font-size: 10px;
    color: var(--text-muted);
    text-align: center;
  }

  /* Supported platforms grid */
  .platforms-title {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0;
    color: var(--text-muted);
    margin-top: 24px;
    margin-bottom: 14px;
  }

  .platforms-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(132px, 1fr));
    gap: 8px;
    width: 100%;
    max-width: 720px;
    margin-top: 4px;
  }

  .platform-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 68px;
    padding: 10px 8px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.025);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.045);
  }

  .platform-card-name {
    font-size: 12px;
    font-weight: 600;
    margin-bottom: 2px;
  }

  .platform-card-domain {
    font-size: 8.5px;
    color: var(--text-muted);
    letter-spacing: 0;
  }

  .platform-card-capability {
    margin-top: 6px;
    max-width: 100%;
    color: var(--text-secondary);
    background: rgba(255, 255, 255, 0.045);
    border-radius: 999px;
    padding: 2px 7px;
    font-size: 8.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @media (max-width: 600px) {
    .window-header {
      padding-right: 14px;
    }

    .paste-section,
    .tabs-nav {
      padding-left: 14px;
      padding-right: 14px;
    }

    .downloads-area {
      padding-left: 14px;
      padding-right: 14px;
    }

    .tabs-list {
      gap: 2px;
      flex: 1;
    }

    .tab-btn {
      justify-content: center;
      padding: 0 9px;
      white-space: nowrap;
    }

    .empty-state {
      padding-left: 16px;
      padding-right: 16px;
    }

    .platforms-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .task-card {
      gap: 10px;
      padding-left: 10px;
      padding-right: 10px;
    }

    .service-icon {
      width: 40px;
      height: 40px;
      flex-basis: 40px;
    }
  }
</style>
