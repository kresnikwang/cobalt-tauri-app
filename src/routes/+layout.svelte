<script lang="ts">
  import '../app.css';
  let { children } = $props();

  // The custom titlebar reserves left space for the macOS traffic-light buttons
  // only inside the native Tauri app. In a plain browser (no traffic lights) the
  // wordmark stays flush with the content column instead of floating mid-row.
  if (typeof window !== 'undefined') {
    const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
    const isMac = /mac/i.test(navigator.platform || navigator.userAgent || '');
    document.documentElement.classList.toggle('native-mac', isTauri && isMac);
  }
</script>

<div id="app">
  {@render children()}
</div>
