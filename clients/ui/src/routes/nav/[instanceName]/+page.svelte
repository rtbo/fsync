<script lang="ts">
  import { daemonNodeAndChildren } from '$lib/model';
  import type types from '$lib/types';
  import {
    Input,
  } from 'flowbite-svelte';
  import { FolderOutline } from 'flowbite-svelte-icons';

  export let data: types.NodeAndChildren;

  export let path = '/';

  let node: types.TreeEntry | null = data.node;
  let children: types.TreeEntry[] = data.children;

  $: updateForPath(path);

  let firstTime = true;
  async function updateForPath(path: string) {
    if (firstTime) {
      firstTime = false;
    } else {
      if (path !== '/') {
        try {
          let res = await daemonNodeAndChildren(path);
          pathInputColor = 'base';
          node = res?.node ?? {};
          children = res?.children ?? [];
        } catch (err) {
          pathInputColor = 'red';
          node = null;
          children = [];
        }
      } else {
        pathInputColor = 'base';
        node = data.node;
        children = data.children;
      }
    }
  }

  $: pathInputValue = path;
  let pathInputColor: 'base' | 'red' = 'base';
</script>

<div class="h-screen w-screen flex flex-col overflow-hidden">
  <nav
    class="bg-white dark:bg-gray-900 w-full z-20 top-0 start-0 border-b border-gray-200 dark:border-gray-600"
  >
    <div class="max-w-screen-xl flex flex-wrap items-center justify-start space-x-6 mx-auto p-4">
      <div class="flex items-center space-x-3 rtl:space-x-reverse">
        <FolderOutline size="lg"></FolderOutline>
      </div>

      <form on:submit|preventDefault={() => (path = pathInputValue)}>
        <Input bind:value={pathInputValue} color={pathInputColor} class="w-96 justify-self-start">
          <span slot="right">
            {pathInputValue === path ? '' : '...'}
          </span>
        </Input>
      </form>
    </div>
  </nav>

  <div class="relative overflow-x-auto flex-grow nav-table">
    <table class="w-full text-sm text-left rtl:text-right text-gray-500 dark:text-gray-400">
      <thead
        class="sticky top-0 text-xs text-gray-700 uppercase bg-gray-50 dark:bg-gray-700 dark:text-gray-400"
      >
        <tr>
          <th scope="col" class="sticky top-0 px-6 py-3"> Name </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> Status </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> Local </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> Remote </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> <span class="sr-only">Actions</span> </th>
        </tr>
      </thead>
      <tbody class="relative overflow-y-auto">
        {#each children as child, idx}
          {@const rowClass = idx < children.length - 1 ? 'border-b dark:border-gray-700' : ''}
          <tr class="max-h-12 dark:bg-gray-800 {rowClass}" >
            <th
              scope="row"
              class="px-6 py-4 font-medium text-gray-900 whitespace-nowrap dark:text-white"
            >
              {child.name}
            </th>
            <td class="px-6 py-4"> </td>
            <td class="px-6 py-4"> </td>
            <td class="px-6 py-4"> </td>
            <td class="px-6 py-4"> </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
