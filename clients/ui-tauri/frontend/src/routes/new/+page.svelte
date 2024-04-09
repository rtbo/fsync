<script lang="ts">
  import { providers, instanceCreate, errorMessage } from '$lib/model';
  import type types from '$lib/types';
  import {
    Button,
    ButtonGroup,
    Input,
    Spinner,
    Label,
    Select,
    Popover,
    Alert
  } from 'flowbite-svelte';
  import { AngleLeftOutline, ArrowUpRightFromSquareOutline } from 'flowbite-svelte-icons';
  import { open } from '@tauri-apps/api/dialog';
  import { goto, afterNavigate } from '$app/navigation';

  let previousPage: string = '';
  afterNavigate(({ from }) => {
    previousPage = from?.url.pathname || previousPage;
  });
  async function back() {
    goto(previousPage ?? '/');
  }

  let errorMsg = '';
  function resetError() {
    errorMsg = '';
  }

  let name = '';
  let namePlaceholder = 'drive';

  let localDir = '';
  let localDirPlaceholder = '';

  import('@tauri-apps/api/path').then(
    async ({ homeDir, join }) =>
      (localDirPlaceholder = await join(await homeDir(), namePlaceholder))
  );

  async function chooseLocalDir() {
    let res = await open({
      title: 'Choose Local Directory',
      directory: true,
      multiple: false
    });
    if (typeof res === 'string') {
      localDir = res;
    }
  }

  let provider: types.Provider = 'drive';

  let driveSecret = 'builtin';
  let driveSecrets = [{ value: 'builtin', name: 'Fsync Built-in client' }];

  let fsRemoteDir = '';
  async function chooseFsRemoteDir() {
    let res = await open({
      title: 'Choose Local Directory',
      directory: true,
      multiple: false
    });
    if (typeof res === 'string') {
      fsRemoteDir = res;
    }
  }

  let spinning = false;

  function makeOpts(): types.ProviderOpts {
    if (provider === 'drive') {
      return {
        drive: {
          root: null,
          secret: 'builtin'
        }
      };
    } else {
      // provider === 'fs'
      return {
        fs: fsRemoteDir
      };
    }
  }

  async function create() {
    try {
      spinning = true;
      errorMsg = '';
      const nam = name !== '' ? name : namePlaceholder;
      const locdir = localDir !== '' ? localDir : localDirPlaceholder;
      await instanceCreate(nam, locdir, makeOpts());
      goto('/connect');
    } catch (err) {
      try {
        errorMsg = await errorMessage(err as types.Error);
      } catch (e) {
        console.error(e);
      }
    } finally {
      spinning = false;
    }
  }
</script>

<div class="container mx-auto flex h-screen">
  <div class="mx-auto my-auto">
    <h1 class="mb-7 text-3xl text-center">Create Instance</h1>
    {#if spinning}
      <div class="min-h-96 flex flex-col items-center">
        <Spinner size="16" class="mt-24" />
      </div>
    {:else}
      <div class="min-h-96">
        <Label class="self-stretch mt-4" id="i-name">
          Pick a name
          <Input class="mt-2" bind:value={name} placeholder={namePlaceholder} on:change={resetError}
          ></Input>
        </Label>

        <Label for="local-dir" class="self-stretch mt-4" id="i-local-dir">
          Local directory
          <ButtonGroup class="w-full mt-2">
            <Input
              id="local-dir"
              bind:value={localDir}
              placeholder={localDirPlaceholder}
              on:change={resetError}
            />
            <Button color="blue" on:click={chooseLocalDir}>Browse</Button>
          </ButtonGroup>
        </Label>
        <Popover class="w-40 text-sm font-light" triggeredBy="#i-local-dir" placement="right">
          Directory on the local filesystem that will be synchronized with the remote drive.
        </Popover>

        <Label class="self-stretch mt-4">
          Provider
          <Select
            placeholder="Choose a provider..."
            class="mt-2"
            items={providers}
            bind:value={provider}
            on:change={resetError}
          ></Select>
        </Label>
        <div class="min-w-96 min-h-28 mt-4">
          {#if provider === 'drive'}
            <Label class="w-full">
              How should FSync connect to Drive?
              <Select
                placeholder="Choose a provider..."
                class="mt-2"
                items={driveSecrets}
                bind:value={driveSecret}
                on:change={resetError}
              ></Select>
            </Label>
          {:else if provider === 'fs'}
            <Label for="remote-dir" class="self-stretch">
              "Remote" directory
              <ButtonGroup class="w-full mt-2">
                <Input id="remote-dir" bind:value={fsRemoteDir} on:change={resetError} />
                <Button color="blue" on:click={chooseFsRemoteDir}>Browse</Button>
              </ButtonGroup>
            </Label>
          {/if}
        </div>
        <div class="w-full flex flex-row justify-around">
          <Button class="mt-2" color="dark" on:click={back}>
            <AngleLeftOutline />&nbsp; Back
          </Button>
          <Button class="mt-2" on:click={create}>
            <ArrowUpRightFromSquareOutline />
            &nbsp; Create
          </Button>
        </div>
      </div>
      {#if errorMsg !== ''}
        <Alert color="red" class="mt-3">
          <span class="font-medium">
            {errorMsg}
          </span>
        </Alert>
      {/if}
    {/if}
  </div>
</div>
