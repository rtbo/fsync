<script lang="ts">
  import { invoke } from "@tauri-apps/api/tauri";
  import { onMount } from "svelte";

  let connected = false;

  onMount(async () => {
    connected = await invoke("connected");
  });
</script>

<main class="container">
  <h1>Welcome to Fsync!</h1>

  <div class="row">
    {#if connected}
      Connected
    {:else}
      Not connected
    {/if}
  </div>
</main>
