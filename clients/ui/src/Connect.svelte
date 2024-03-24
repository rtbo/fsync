<script>
  import { invoke } from "@tauri-apps/api";

  let instancesPromise = invoke("instances_get_all").then((data) => {
    console.log(data);
    return data;
  });

  let newProvider = "GoogleDrive";
  const providers = new Map([
    [ "GoogleDrive", "Google Drive" ],
    [ "LocalFs", "Local Filesystem" ],
  ]);
</script>

<div class="flex flex-row space-x-4">
  <div class="flex-col basis-2/3 space-y-2">
    {#await instancesPromise}
      ...Loading
    {:then instances}
      {#each instances as instance}
        <div class="panel flex justify-between">
          <div class="flex-col">
            <p class="text-lg font-mono">
              {instance.name}
              {#if instance.running}
                <span class="text-dim">(running)</span>
              {/if}
            </p>
            <p class="text-dim text-sm">
              <span class="underline">path:</span>
              <span class="font-mono text-not-dim">{instance.local_dir}</span>
            </p>
            <p class="text-dim text-sm">
              <span class="underline">provider:</span>
              <span class="font-mono text-not-dim">{providers.get(instance.provider)}</span>
            </p>
          </div>
          <button class="btn-primary self-center">Connect</button>
        </div>
      {/each}
    {/await}
  </div>
  <div>
    <t1>Create New Fsync Instance</t1>
    <select bind:value={newProvider}>
      <option value="GoogleDrive">Google Drive</option>
      <option value="LocalFs">Local Filesystem</option>
    </select>
    <button>Create</button>
  </div>
</div>
