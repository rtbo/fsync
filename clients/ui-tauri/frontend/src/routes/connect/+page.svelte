<script lang="ts">
  import { selectName } from '$lib/utils';
  import { providers } from '$lib/model';
  import { Button, Card } from 'flowbite-svelte';
  import { ArrowUpRightFromSquareOutline, PlusOutline } from 'flowbite-svelte-icons';
  import type { PageData } from './$types';

  export let data: PageData;
</script>

<div class="container mx-auto flex h-screen">
  <div class="mx-auto my-auto">
    <h1 class="mb-7 text-3xl text-center">Connect to instance</h1>

    <div class="flex flex-col mb-4">
      <div class="grid-cols-3 sm:grid-cols-1 space-y-4">
        {#each data.instances as instance}
          <Card size="lg" padding="sm">
            <div class="flex flex-row justify-between">
              <div class="mr-8">
                <h5
                  class="mb-2 text-2xl font-bold font-mono tracking-tight text-gray-900 dark:text-white"
                >
                  {instance.name}
                </h5>
                <p class="mb-3 font-normal text-gray-700 dark:text-gray-300">
                  <span class="underline">path</span>:
                  <span class="font-mono">{instance.localDir}</span>
                </p>
                <p class="mb-3 font-normal text-gray-700 dark:text-gray-300">
                  <span class="underline">provider</span>:
                  {selectName(instance.provider, providers)}
                </p>
              </div>
              {#if instance.running}
                <Button class="w-fit ml-3 self-center" href={'/nav/' + instance.name}>
                  <ArrowUpRightFromSquareOutline />&nbsp; Connect
                </Button>
              {:else}
                <span class="text-sm font-sans font-normal tracking-normal opacity-75 mx-4">
                  (not running)
                </span>
              {/if}
            </div>
          </Card>
        {/each}
      </div>
    </div>
    <Button href="/new" pill={true} size="lg" class="!p-2 fixed end-8 bottom-8">
      <PlusOutline class="w-8 h-8" />
    </Button>
  </div>
</div>
