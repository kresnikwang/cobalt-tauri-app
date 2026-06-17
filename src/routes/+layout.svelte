<script lang="ts">
  import '../app.css';
  let { children } = $props();
</script>

<div id="app">
  {@render children()}
</div>
