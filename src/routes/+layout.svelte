<script lang="ts">
  import '../app.css';
  let { children } = $props();

  // `titleBarStyle: "Overlay"` draws the webview full-size (behind the macOS
  // traffic lights), so the app renders its own header and reserves room.
  if (typeof window !== 'undefined') {
    const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
    const isMac = /mac/i.test(navigator.platform || navigator.userAgent || '');
    document.documentElement.classList.toggle('native-mac', isTauri && isMac);
  }
</script>

<div id="app">
  {@render children()}
</div>
