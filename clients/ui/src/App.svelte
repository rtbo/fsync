<script>
  import { invoke } from "@tauri-apps/api/tauri";
  import { onMount } from "svelte";
  import Connect from "./Connect.svelte";

  let connected = false;

  onMount(async () => {
    connected = await invoke("daemon_connected");
  });
</script>

<main class="container mx-auto flex h-full">
    {#if connected}
      Connected
    {:else}
      <div class="mx-auto">
        <p class="text-center text-xl my-12">Not connected</p>
        <Connect />
      </div>
    {/if}
</main>
