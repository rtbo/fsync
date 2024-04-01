import { invoke } from '@tauri-apps/api';
import type types from './types';
import type { SelectOptionType } from 'flowbite-svelte';

export const providers: SelectOptionType<types.Provider>[] = [
  { value: 'drive', name: 'Google Drive' },
  { value: 'fs', name: 'Local Filesystem' }
];

export async function errorMessage(error: types.Error): Promise<string> {
  return await invoke('error_message', {
    error
  });
}

export async function instanceGetAll(): Promise<types.Instance[]> {
  return invoke('instance_get_all');
}

export async function instanceCreate(name: string, localDir: string, opts: types.ProviderOpts) {
  const args = {
    name,
    localDir,
    opts
  };
  invoke('instance_create', args);
}

export async function daemonConnected(): Promise<boolean> {
  return invoke('daemon_connected');
}

export async function daemonInstanceName(): Promise<boolean> {
  return invoke('daemon_connected');
}

export async function daemonConnect(name?: string): Promise<void> {
  return invoke('daemon_connect', {
    name: name ?? null
  });
}

export async function daemonNodeAndChildren(path: string | null): Promise<types.NodeAndChildren> {
  return invoke('daemon_node_and_children', { path });
}

export function metadataEntryType(metadata: types.Metadata): types.EntryType {
  if ('directory' in metadata) {
    return 'directory';
  } else {
    return 'regular';
  }
}

export function entryType(entry: types.Entry | types.TreeEntry): types.EntryType {
  if ('entry' in entry) {
    return entryType(entry.entry);
  }

  if ('local' in entry) {
    return metadataEntryType(entry.local);
  } else if ('remote' in entry) {
    return metadataEntryType(entry.remote);
  } else {
    const sync = entry.sync;
    const local = metadataEntryType(sync.local);
    const remote = metadataEntryType(sync.remote);
    if (local !== remote) {
      return 'inconsistent';
    }
    return local;
  }
}
