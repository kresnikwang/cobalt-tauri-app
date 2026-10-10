<script lang="ts">
  import '../app.css';
  let { children } = $props();

  // The transparent macOS titlebar already sits above the webview, so use a
  // compact app header while keeping the workspace's shared left edge.
  if (typeof window !== 'undefined') {
    const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
    const isMac = /mac/i.test(navigator.platform || navigator.userAgent || '');
    document.documentElement.classList.toggle('native-mac', isTauri && isMac);
  }
</script>

<div id="app">
  {@render children()}
</div>
