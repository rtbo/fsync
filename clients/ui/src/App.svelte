<script>
  import { invoke } from "@tauri-apps/api/tauri";
  import { onMount } from "svelte";
    import Connect from "./Connect.svelte";

  let connected = false;

  onMount(async () => {
    connected = await invoke("daemon_connected");
  });
</script>

<main class="container">
  <h1>Welcome to Fsync!</h1>

  <div class="row">
    {#if connected}
      Connected
    {:else}
      Not connected
      <Connect />
    {/if}
  </div>
</main>
