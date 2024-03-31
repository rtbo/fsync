<script lang="ts">
  import { selectName } from '$lib/utils';
  import { instanceGetAll, providers } from '$lib/api';
  import { Button, Card } from 'flowbite-svelte';
  import type types from '$lib/types';

  let instances: types.Instance[] = [];

  async function updateInstances() {
    instances = await instanceGetAll();
  }

  updateInstances();
</script>

<div class="mx-auto my-auto">
  <h1 class="my-7 text-3xl text-center">Connect to instance</h1>

  <div class="flex flex-col">
    {#await instancesPromise}
      ...Loading
    {:then instances}
      <div class="grid-cols-3 sm:grid-cols-1">
        {#each instances as instance}
          <Card>
            <div class="flex flex-row">
              <div>
                <h5
                  class="mb-2 text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white"
                >
                  {instance.name}
                  {#if instance.running}
                    <span class="text-sm font-sans font-normal tracking-normal opacity-75"
                      >(running)</span
                    >
                  {/if}
                </h5>
                <p class="mb-3 font-normal text-gray-700 dark:text-gray-300">
                  <span class="underline">path:</span>
                <span class="font-mono">{instance.localDir}</span>
                </p>
                <p class="mb-3 font-normal text-gray-700 dark:text-gray-300">
                  <span class="underline">provider:</span>
                  {selectName(instance.provider, providers)}
                </p>
              </div>
              <Button class="w-fit ml-3 self-center">Connect</Button>
            </div>
          </Card>
        {/each}
      </div>

      <hr class="my-8 h-0.5 border-t-0 bg-neutral-100 dark:bg-white/10" />
    {/await}

    <Button href="/new" class="self-center">Create New</Button>
  </div>
</div>
