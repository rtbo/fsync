import type types from './types';
import type { SelectOptionType } from 'flowbite-svelte';

export const providers: SelectOptionType<types.Provider>[] = [
  { value: 'drive', name: 'Google Drive' },
  { value: 'fs', name: 'Local Filesystem' }
];

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

export type EntryStatus = 'local' | 'remote' | 'sync' | 'conflict';

export function entryStatus(entry: types.Entry | types.TreeEntry): EntryStatus {
  if ('entry' in entry) {
    return entryStatus(entry.entry);
  }

  if ('local' in entry) {
    return 'local';
  } else if ('remote' in entry) {
    return 'remote';
  } else {
    // sync
    const sync = entry.sync;
    if (sync.conflict) {
      return 'conflict';
    } else {
      return 'sync';
    }
  }
}

export type EntrySize =
  | number
  | {
      local: number;
      remote: number;
    };

export function metadataSize(metadata: types.Metadata): number {
  if ('directory' in metadata) {
    return metadata.directory.stat?.data ?? 0;
  } else {
    return metadata.regular.size;
  }
}

export function entrySize(entry: types.Entry | types.TreeEntry): EntrySize {
  if ('entry' in entry) {
    return entrySize(entry.entry);
  }

  if ('local' in entry) {
    return metadataSize(entry.local);
  }
  if ('remote' in entry) {
    return metadataSize(entry.remote);
  }
  const local = metadataSize(entry.sync.local);
  const remote = metadataSize(entry.sync.remote);
  if (local === remote) {
    return local;
  } else {
    return {
      local,
      remote
    };
  }
}
