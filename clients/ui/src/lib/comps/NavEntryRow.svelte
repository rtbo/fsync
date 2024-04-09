<script lang="ts">
  import { daemonOperate } from '$lib/ipc';
  import { entryStatus, entryType, type EntryStatus, entrySize } from '$lib/model';
  import type types from '$lib/types';
  import { createEventDispatcher } from 'svelte';
  import MatSymIcon from './MatSymIcon.svelte';
  import { Progressbar, Spinner } from 'flowbite-svelte';
  import prettyBytes from 'pretty-bytes';

  let addedClass = '';
  export { addedClass as class };

  export let entry: types.TreeEntry;

  function entryStatusIcon(status: EntryStatus): [string, string] {
    switch (status) {
      case 'local':
        return ['text-gray-800 dark:text-gray-400', 'hard_drive'];
      case 'remote':
        return ['text-cyan-600 dark:text-cyan-400', 'cloud'];
      case 'sync':
        return ['text-green-600 dark:text-green-400', 'check_circle'];
      case 'conflict':
        return ['text-red-600 dark:text-red-400', 'error'];
    }
  }

  $: etyp = entryType(entry);
  $: typeIcon = etyp === 'directory' ? 'folder' : 'draft';
  $: nameClass = etyp === 'directory' ? 'cursor-pointer' : '';
  $: status = entryStatus(entry);
  $: [statusClass, statusIcon] = entryStatusIcon(status);
  $: size = entrySize(entry);

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

  async function copy(dir: types.StorageDir) {
    const prog = await daemonOperate({
      copy: [entry.path, dir]
    });
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

<tr class="h-12 dark:bg-gray-800 {addedClass}">
  <td class="px-2 pt-1 text-center align-middle text-gray-900 dark:text-white">
    <span class="material-symbols-outlined font-medium">{typeIcon}</span>
  </td>
  <th
    scope="row"
    class="pl-0 pr-6 text-left align-middle font-medium text-gray-900 whitespace-nowrap dark:text-white {nameClass}"
    on:dblclick={() => childDoubleClick()}
  >
    {entry.name}
  </th>
  <td class="px-6 text-center align-middle pt-1 font-medium">
    <span class="material-symbols-outlined font-medium {statusClass}">{statusIcon}</span>
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
  <td class="px-6 py-2"> </td>
  <td class="px-6 pt-1">
    {#if progressPercent === 'spin'}
      <Spinner />
    {:else if progressPercent !== null}
      <Progressbar progress={progressPercent} />
    {:else if status === 'local'}
      <button on:click={() => copy('localToRemote')}>
        <MatSymIcon>upload</MatSymIcon>
      </button>
    {:else if status === 'remote'}
      <button on:click={() => copy('remoteToLocal')}>
        <MatSymIcon>download</MatSymIcon>
      </button>
    {/if}
  </td>
</tr>