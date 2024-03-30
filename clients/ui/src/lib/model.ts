import { invoke } from '@tauri-apps/api';
import type types from './types';
import type { SelectOptionType } from 'flowbite-svelte';

export const providers: SelectOptionType<types.Provider>[] = [
  { value: 'drive', name: 'Google Drive' },
  { value: 'fs', name: 'Local Filesystem' }
];

export async function newCreateConfig(name: string, localDir: string, opts: types.ProviderOpts) {
  const args = {
    name,
    localDir,
    opts
  };
  invoke('instances_create_config', args);
}
