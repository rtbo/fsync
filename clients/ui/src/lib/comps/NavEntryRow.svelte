<script lang="ts">
  import { daemonOperate } from '$lib/ipc';
  import { entryStatus, entryType, type EntryStatus, entrySize, entryMtime } from '$lib/model';
  import type types from '$lib/types';
  import { createEventDispatcher } from 'svelte';
  import MatSymIcon from './MatSymIcon.svelte';
  import { Progressbar, Spinner } from 'flowbite-svelte';
  import prettyBytes from 'pretty-bytes';
    import { showContextMenu } from '$lib/context-menu';

  let addedClass = '';
  export { addedClass as class };

  export let entry: types.TreeEntry;

  function entryStatusIcon(status: EntryStatus): [string, string] {
    switch (status) {
      case 'local':
        return ['text-gray-500 dark:text-gray-400', 'hard_drive'];
      case 'remote':
        return ['text-cyan-500 dark:text-cyan-400', 'cloud'];
      case 'sync':
        return ['text-gray-500 dark:text-gray-400', 'check_circle'];
      case 'syncFull':
        return ['text-green-500 dark:text-green-400', 'check_circle'];
      case 'conflict':
        return ['text-gray-500 dark:text-gray-400', 'error'];
      case 'conflictFull':
        return ['text-red-600 dark:text-red-400', 'error'];
    }
  }

  $: etyp = entryType(entry);
  $: typeIcon = etyp === 'directory' ? 'folder' : 'draft';
  $: nameClass = etyp === 'directory' ? 'cursor-pointer' : '';
  $: status = entryStatus(entry);
  $: [statusClass, statusIcon] = entryStatusIcon(status);
  $: size = entrySize(entry);
  $: mtime = entryMtime(entry);

  function displayMtime(mtime: number | null): string {
    if (mtime === null) {
      return '';
    }
    const now = new Date(Date.now());
    const date = new Date(mtime);

    const today =
      now.getDate() == date.getDate() &&
      now.getMonth() == date.getMonth() &&
      now.getFullYear() == date.getFullYear();
    if (today) {
      return date.toLocaleTimeString();
    }
    return date.toLocaleDateString();
  }

  function displayDiffMtime(
    local: number | null,
    remote: number | null
  ): { local: string; remote: String } {
    if (local === null || remote === null) {
      return { local: displayMtime(local), remote: displayMtime(remote) };
    }

    const localDate = new Date(local);
    const remoteDate = new Date(remote);

    const sameDay =
      localDate.getDate() === remoteDate.getDate() &&
      localDate.getMonth() === remoteDate.getMonth() &&
      localDate.getFullYear() === remoteDate.getFullYear();

    if (sameDay) {
      const now = new Date(Date.now());
      const today =
        now.getDate() == localDate.getDate() &&
        now.getMonth() == localDate.getMonth() &&
        now.getFullYear() == localDate.getFullYear();
      if (today) {
        return { local: localDate.toLocaleTimeString(), remote: remoteDate.toLocaleTimeString() };
      } else {
        return { local: localDate.toLocaleString(), remote: remoteDate.toLocaleString() };
      }
    } else {
      return { local: displayMtime(local), remote: displayMtime(remote) };
    }
  }

  const dispatch = createEventDispatcher();

  function computeProgressPercent(p: types.PathProgress[]): number | null | 'spin' {
    if (p.length === 0) {
      return null;
    }
    let done = 0;
    let total = 0;
    p.forEach((pp: types.PathProgress) => {
      if (typeof pp.progress === 'object' && 'progress' in pp.progress) {
        const ppp = pp.progress.progress;
        done += ppp.progress;
        total += ppp.total;
      }
    });
    if (total === 0) {
      return 'spin';
    }
    return 100 * (done / total);
  }

  // let inProgress = false;

  // function checkProgressDone(p: types.PathProgress[]) {
  //   if (p.length === 0 && inProgress) {
  //     inProgress = false;
  //     console.log('progress done', entry.path);
  //     dispatch('mutation');
  //   }
  //   inProgress = p.length !== 0;
  // }

  export let progress: types.PathProgress[];
  $: progressPercent = computeProgressPercent(progress);

  function childDoubleClick() {
    if (etyp === 'directory') {
      dispatch('navigate', {
        path: entry.path
      });
    }
  }

  async function sync() {
    const op: types.Operation = etyp === 'directory' ? {
      syncDeep: entry.path,
    } : {
      sync: entry.path
    };
    return operate(op);
  }

  // async function resolve() {
  // }

  async function contextMenu(): Promise<void> {
    await showContextMenu(entry, etyp, status, operate);
  }

  async function operate(op: types.Operation) {
    const prog = await daemonOperate(op);
    if (prog === 'done') {
      dispatch('mutation');
    } else {
      dispatch('progress', {
        path: entry.path,
        progress: prog
      });
    }
  }
</script>

<tr class="h-12 dark:bg-gray-800 select-none {addedClass}" on:contextmenu|preventDefault={contextMenu}>
  <td
    class="px-2 pt-1 text-center align-middle text-gray-900 dark:text-white {nameClass}"
    on:dblclick={() => childDoubleClick()}
  >
    <MatSymIcon class="font-medium">{typeIcon}</MatSymIcon>
  </td>
  <th
    scope="row"
    class="pl-0 pr-6 text-left align-middle font-medium text-gray-900 whitespace-nowrap dark:text-white {nameClass}"
    on:dblclick={() => childDoubleClick()}
  >
    {entry.name}
  </th>
  <td class="px-6 text-center align-middle pt-1 font-medium">
    <MatSymIcon class="font-medium {statusClass}">{statusIcon}</MatSymIcon>
  </td>
  <td class="px-2 py-0">
    <div class="text-start align-middle">
      {#if typeof size === 'number'}
        <span class="ml-7">{prettyBytes(size)}</span>
      {:else}
        <p class="text-sm">
          <MatSymIcon class="align-middle font-extralight mr-1 text-xl/5">hard_drive</MatSymIcon>
          <span class="align-middle">{prettyBytes(size.local)}</span>
        </p>
        <p class="text-sm">
          <MatSymIcon class="align-middle font-extralight mr-1 text-xl/5">cloud</MatSymIcon>
          <span class="align-middle">{prettyBytes(size.remote)}</span>
        </p>
      {/if}
    </div>
  </td>
  <td class="px-2 py-0">
    <div class="text-start align-middle">
      {#if typeof mtime === 'number'}
        <span class="ml-7">{displayMtime(mtime)}</span>
      {:else if mtime === null}
        <span></span>
      {:else}
        {@const { local, remote } = displayDiffMtime(mtime.local, mtime.remote)}
        <p class="text-sm">
          <MatSymIcon class="align-middle font-extralight mr-1 text-xl/5">hard_drive</MatSymIcon>
          <span class="align-middle">{local}</span>
        </p>
        <p class="text-sm">
          <MatSymIcon class="align-middle font-extralight mr-1 text-xl/5">cloud</MatSymIcon>
          <span class="align-middle">{remote}</span>
        </p>
      {/if}
    </div>
  </td>
  <td class="px-6 pt-1">
    {#if progressPercent === 'spin'}
      <Spinner size="6" />
    {:else if progressPercent !== null}
      <Progressbar progress={progressPercent} />
    {:else if status === 'local'}
      <button on:click={() => sync()}>
        <MatSymIcon>upload</MatSymIcon>
      </button>
    {:else if status === 'remote'}
      <button on:click={() => sync()}>
        <MatSymIcon>download</MatSymIcon>
      </button>
    {:else if status === 'conflict' || status === 'conflictFull'} 
      <!-- <button on:click={() => resolve()}>
        <MatSymIcon>sync_problem</MatSymIcon>
      </button>  -->
    {/if}
  </td>
</tr>
