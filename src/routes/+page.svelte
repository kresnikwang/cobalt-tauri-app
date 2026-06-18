<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
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

  async function restartApp() {
    await invoke('restart_app');
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
    <div class="drop-overlay">
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
      <button class="settings-btn" onclick={() => showSettings = !showSettings} title={t('settings.title')}>
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
        class="url-input"
      />
      <button class="download-trigger-btn" onclick={() => handleDownload()} disabled={!inputUrl}>
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
        {t('tabs.all')} <span>{tasks.length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'downloading'} onclick={() => activeTab = 'downloading'}>
        {t('tabs.downloading')} <span>{tasks.filter(t => ['downloading', 'analyzing', 'merging'].includes(t.status)).length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'completed'} onclick={() => activeTab = 'completed'}>
        {t('tabs.completed')} <span>{tasks.filter(t => t.status === 'completed').length}</span>
      </button>
      <button class="tab-btn" class:active={activeTab === 'failed'} onclick={() => activeTab = 'failed'}>
        {t('tabs.failed')} <span>{tasks.filter(t => ['failed', 'cancelled'].includes(t.status)).length}</span>
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
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div 
              class="platform-card" 
              style="--color-glow: {platform.color}; --bg-glow: {platform.bg}"
              onclick={() => inputUrl = `https://${platform.domain}/`}
              title="Click to paste brand domain"
            >
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
    <div class="clipboard-toast glass">
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
    <div class="settings-backdrop" onclick={() => showSettings = false}>
      <div class="settings-panel glass" onclick={(e) => e.stopPropagation()}>
        <div class="settings-header">
          <h3>{t('settings.title')}</h3>
          <button class="close-settings" onclick={() => showSettings = false}>
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
              <button class="btn-select-dir" onclick={selectDirectory}>
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
    border-radius: 20px;
    padding: 40px;
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
  }

  .drop-card p {
    color: var(--text-secondary);
    margin: 0;
  }

  /* Window Header */
  .window-header {
    height: 52px;
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
    letter-spacing: 0.5px;
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
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s;
  }

  .settings-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }

  /* URL Paste Section */
  .paste-section {
    padding: 22px 20px 14px 20px;
  }

  .input-glow-wrapper {
    display: flex;
    align-items: center;
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 14px;
    padding: 4px 4px 4px 16px;
    transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.3);
  }

  .input-glow-wrapper:focus-within {
    border-color: var(--accent-primary);
    box-shadow: 0 0 15px rgba(99, 102, 241, 0.3), inset 0 1px 2px rgba(0,0,0,0.4);
  }

  .url-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 14px;
    height: 38px;
  }

  .url-input::placeholder {
    color: var(--text-muted);
  }

  .download-trigger-btn {
    background: var(--accent-gradient);
    border: none;
    color: white;
    padding: 0 18px;
    height: 38px;
    border-radius: 10px;
    font-weight: 600;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    transition: all 0.2s;
    box-shadow: 0 4px 12px rgba(99, 102, 241, 0.3);
  }

  .download-trigger-btn:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 6px 16px rgba(99, 102, 241, 0.4);
  }

  .download-trigger-btn:active:not(:disabled) {
    transform: translateY(0);
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
    border: 1px solid rgba(255, 255, 255, 0.055);
    font-size: 10.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .context-pill.highlight {
    color: var(--accent-primary);
    background: rgba(99, 102, 241, 0.1);
    border-color: rgba(99, 102, 241, 0.18);
  }

  /* Tabs Nav */
  .tabs-nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 20px;
    margin-bottom: 12px;
  }

  .tabs-list {
    display: flex;
    gap: 8px;
    background: rgba(0, 0, 0, 0.2);
    padding: 3px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.04);
  }

  .tab-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    border-radius: 7px;
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tab-btn.active {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }

  .tab-btn span {
    background: rgba(255, 255, 255, 0.08);
    padding: 1px 6px;
    border-radius: 10px;
    font-size: 10px;
    color: var(--text-secondary);
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
    transition: all 0.2s;
  }

  .clear-btn:hover {
    color: var(--text-secondary);
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
    border-radius: 12px;
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
    border-radius: 12px;
    color: var(--service-color);
    background: var(--service-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid rgba(255, 255, 255, 0.055);
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
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    opacity: 0.85;
  }

  .task-status-badge {
    flex: 0 0 auto;
    font-size: 8px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 6px;
    letter-spacing: 0.5px;
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
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 1px solid var(--border-color);
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: all 0.2s;
  }

  .action-circle-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
    border-color: var(--border-hover);
    transform: scale(1.05);
  }

  .action-circle-btn.danger:hover {
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
    border-color: rgba(239, 68, 68, 0.3);
  }

  .action-circle-btn.success:hover {
    background: rgba(16, 185, 129, 0.15);
    color: #10b981;
    border-color: rgba(16, 185, 129, 0.3);
  }

  /* Empty State */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 44px 40px 34px;
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
    border: 1px solid rgba(255, 255, 255, 0.04);
  }

  .empty-state h3 {
    font-family: var(--font-display);
    font-size: 18px;
    margin: 0 0 8px 0;
    font-weight: 600;
  }

  .empty-state p {
    color: var(--text-secondary);
    font-size: 13px;
    width: min(100%, 520px);
    text-wrap: balance;
    margin: 0 0 32px 0;
    line-height: 1.5;
  }

  /* Clipboard Toast */
  .clipboard-toast {
    position: absolute;
    bottom: 20px;
    left: 20px;
    right: 20px;
    border-radius: 14px;
    padding: 14px 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    z-index: 500;
    box-shadow: var(--shadow-lg);
    animation: slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1);
    background: rgba(22, 22, 33, 0.95);
    border-color: rgba(99, 102, 241, 0.3);
  }

  @keyframes slideUp {
    0% { transform: translateY(40px); opacity: 0; }
    100% { transform: translateY(0); opacity: 1; }
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
  }

  .toast-text p {
    margin: 0;
    font-size: 11px;
    color: var(--text-secondary);
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
    padding: 6px 12px;
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s;
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
    width: 320px;
    height: 100%;
    background: rgba(20, 20, 30, 0.85);
    border-left: 1px solid var(--border-color);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    animation: slideIn 0.3s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes slideIn {
    0% { transform: translateX(100%); }
    100% { transform: translateX(0); }
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
  }

  .close-settings {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .close-settings:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
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
    letter-spacing: 0.5px;
  }

  .path-selector {
    display: flex;
    gap: 6px;
  }

  .path-input {
    flex: 1;
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 32px;
    outline: none;
    text-overflow: ellipsis;
  }

  .btn-select-dir {
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    width: 32px;
    height: 32px;
    border-radius: 8px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .btn-select-dir:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .settings-select {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 32px;
    outline: none;
  }

  .checkbox-item {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    margin-top: 10px;
  }

  .checkbox-item input {
    margin: 0;
    cursor: pointer;
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
    letter-spacing: 1px;
    color: var(--accent-primary);
    margin-top: 4px;
  }

  .settings-input {
    background: var(--bg-input);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-primary);
    padding: 0 10px;
    font-size: 12px;
    height: 32px;
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
    font-size: 9px;
    color: var(--text-muted);
    font-style: italic;
  }

  .restart-btn {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
    width: 100%;
  }

  .restart-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    border-color: var(--accent-primary);
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
    letter-spacing: 1.5px;
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
    min-height: 62px;
    padding: 9px 8px;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.03);
    background: rgba(255, 255, 255, 0.02);
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.25, 1, 0.5, 1);
  }

  .platform-card:hover {
    background: rgba(255, 255, 255, 0.05);
    border-color: var(--color-glow);
    box-shadow: 0 4px 12px var(--bg-glow);
    transform: translateY(-1px);
  }

  .platform-card:active {
    transform: translateY(0);
  }

  .platform-card-name {
    font-size: 11.5px;
    font-weight: 600;
    margin-bottom: 2px;
  }

  .platform-card-domain {
    font-size: 8.5px;
    color: var(--text-muted);
    letter-spacing: 0.2px;
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
</style>
