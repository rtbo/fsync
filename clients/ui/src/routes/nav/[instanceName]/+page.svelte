<script lang="ts">
  import { MatSymIcon, NavEntryRow } from '$lib/comps';
  import { daemonNodeAndChildren, } from '$lib/ipc';
  import { createProgressesStore } from '$lib/progress';
  import type types from '$lib/types';
  import { Input } from 'flowbite-svelte';

  export let data: types.NodeAndChildren;

  export let path = '/';

  let node: types.TreeEntry | null = data.node;
  let children: types.TreeEntry[] = data.children;

  $: progress = createProgressesStore(path, ackMutation);

  $: updateForPath(path);

  let firstTime = true;
  async function updateForPath(path: string) {
    if (firstTime) {
      firstTime = false;
      return;
    }

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

  // historyIndex points to the current path
  // pathHistory[.. historyIndex] is back history
  // pathHistory[historyIndex + 1 ..] is next history
  let pathHistory: string[] = [path];
  let historyIndex = 0;

  function navigate(newPath: string) {
    // delete next history
    if (pathHistory.length > 0 && historyIndex < pathHistory.length - 1) {
      pathHistory = pathHistory.slice(0, historyIndex + 1);
    }
    pathHistory = [...pathHistory, newPath];
    historyIndex = pathHistory.length - 1;

    path = newPath;
  }

  function goBack() {
    if (pathHistory.length > 0 && historyIndex > 0) {
      historyIndex -= 1;
      path = pathHistory[historyIndex];
    }
  }

  function goNext() {
    if (pathHistory.length > 0 && historyIndex < pathHistory.length - 1) {
      historyIndex += 1;
      path = pathHistory[historyIndex];
    }
  }

  let goUp = async () => {};
  // avoid static import bug
  // https://github.com/tauri-apps/tauri/issues/9324
  import('@tauri-apps/api/path').then(({ dirname }) => {
    goUp = async () => {
      navigate(await dirname(path));
    };
  });

  async function goHome() {
    navigate('/');
  }

  async function ackMutation() {
    data = await daemonNodeAndChildren('/');
    await updateForPath(path);
  }

  $: backEnabled = pathHistory.length > 1 && historyIndex > 0;
  $: nextEnabled = pathHistory.length > 1 && historyIndex < pathHistory.length - 1;
  $: upEnabled = path !== '/';

  $: pathInputValue = path;

  let pathInputColor: 'base' | 'red' = 'base';
</script>

<div class="h-screen w-screen flex flex-col overflow-hidden">
  <nav
    class="bg-white dark:bg-gray-900 w-full z-20 top-0 start-0 border-b border-gray-200 dark:border-gray-600"
  >
    <div class="max-w-screen-xl flex flex-wrap items-center justify-start space-x-6 mx-auto p-4">
      <a href="/connect" class="flex items-center space-x-3 rtl:space-x-reverse"> FS </a>

      <button
        on:click={goBack}
        class={backEnabled ? 'cursor-pointer' : 'opacity-50'}
        disabled={!backEnabled}
      >
        <MatSymIcon> chevron_left </MatSymIcon>
      </button>

      <button
        on:click={goNext}
        class={nextEnabled ? 'cursor-pointer' : 'opacity-50'}
        disabled={!nextEnabled}
      >
        <MatSymIcon> chevron_right </MatSymIcon>
      </button>

      <button
        on:click={goUp}
        class={upEnabled ? 'cursor-pointer' : 'opacity-50'}
        disabled={!upEnabled}
      >
        <MatSymIcon> expand_less </MatSymIcon>
      </button>

      <button
        class={upEnabled ? 'cursor-pointer' : 'opacity-50'}
        on:click={goHome}
        disabled={!upEnabled}
      >
        <MatSymIcon> home </MatSymIcon>
      </button>

      <form on:submit|preventDefault={() => navigate(pathInputValue)}>
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
          <th scope="col" class="sticky top-0 px-2 py-3"> <span class="sr-only">Type</span> </th>
          <th scope="col" class="sticky top-0 pl-0 pr-6 py-3"> Name </th>
          <th scope="col" class="sticky top-0 px-6 py-3 text-center"> Status </th>
          <th scope="col" class="sticky top-0 px-2 py-3 min-w-28 w-28 text-start"> <span class="ml-7"> Size </span> </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> Modified </th>
          <th scope="col" class="sticky top-0 px-6 py-3"> <span class="sr-only">Actions</span> </th>
        </tr>
      </thead>
      <tbody class="relative overflow-y-auto">
        {#each children as entry, idx}
          {@const borderClass = idx < children.length - 1 ? 'border-b dark:border-gray-700' : ''}
          <NavEntryRow
            {entry}
            class={borderClass}
            progress={$progress.filter((p) => p.path.startsWith(entry.path))}
            on:progress={(e) => progress.add(e.detail)}
            on:mutation={ackMutation}
            on:navigate={(e) => navigate(e.detail.path)}
          />
        {/each}
      </tbody>
    </table>
  </div>
</div>
