<script lang="ts">
  import { daemonOperate, daemonProgresses, entryStatus, entryType, type EntryStatus } from '$lib/model';
  import type types from '$lib/types';
  import { createEventDispatcher } from 'svelte';
  import MatSymIcon from './MatSymIcon.svelte';
  import { Progressbar } from 'flowbite-svelte';

  let addedClass = '';
  export { addedClass as class };

  export let entry: types.TreeEntry;

  function entryStatusIcon(status: EntryStatus): [string, string] {
    switch (status) {
      case 'local':
        return ['text-gray-800 dark:text-gray-200', 'file_save'];
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

  const dispatch = createEventDispatcher();

  let progress: types.PathProgress[] = [];
  let progressInterval: number | undefined = undefined;

  function listenProgress() {
    if (progressInterval === undefined) {
      progressInterval = setInterval(async () => {
        progress = await daemonProgresses(entry.path);
        if (progress.length === 0) {
          clearInterval(progressInterval);
          progressInterval = undefined;
          dispatch('mutation');
        }
      }, 200);
    }
  }

  function computeProgressPercent(p: types.PathProgress[]): number | null {
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
    return 100 * (done / total);
  }

  $: progressPercent = computeProgressPercent(progress);

  function childDoubleClick() {
    if (etyp === 'directory') {
      dispatch('navigate', {
        path: entry.path,
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
      listenProgress();
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
  <td class="px-6 py-4"> </td>
  <td class="px-6 py-4"> </td>
  <td class="px-6 pt-1">
    {#if progressPercent !== null}
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
