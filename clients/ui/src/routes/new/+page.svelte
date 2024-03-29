<script lang="ts">
  import { providers } from '$lib/model';
  import { Button, Label, Select } from 'flowbite-svelte';

  let newProvider = 'drive';

  let driveSecret = 'builtin';
  let driveSecrets = [
    { value: 'builtin', name: 'Fsync Built-in client' },
    { value: 'json', name: 'JSON client file' },
    { value: 'creds', name: 'Client ID and Secret' }
  ];
</script>

<div class="mx-auto my-auto flex flex-col">
  <h1 class="mb-7 text-3xl text-center">Create Instance</h1>
  <div>
    <Label>
      Provider
      <Select
        placeholder="Choose a provider..."
        class="mt-2"
        items={providers}
        bind:value={newProvider}
      ></Select>
    </Label>
    {#if newProvider == 'GoogleDrive'}
      <Label class="my-3">
        How should FSync connect to Drive?
        <Select
          placeholder="Choose a provider..."
          class="mt-2"
          items={driveSecrets}
          bind:value={driveSecret}
        ></Select>
      </Label>
    {/if}
  </div>
  <Button class="self-end mt-2">Create</Button>
</div>
