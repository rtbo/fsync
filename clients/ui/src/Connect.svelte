<script>
  import { invoke } from "@tauri-apps/api";

  let instancesPromise = invoke("instances_get_all").then((data) => {
    console.log(data);
    return data;
  });

  let newProvider = "GoogleDrive";
</script>

<div>
  <row>
    <div>
      {#await instancesPromise}
        Loading...
      {:then instances}
        {#each instances as instance}
          <p>
            {instance.name}
            {#if instance.running}
              (Running)
            {/if}
            <button>Connect</button>
          </p>
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
  </row>
</div>
