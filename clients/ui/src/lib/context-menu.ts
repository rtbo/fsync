import { Menu, MenuItem, Submenu } from '@tauri-apps/api/menu';
import type types from './types';
import type { EntryStatus } from './model';
import { openPath } from './ipc';

export type OperateCb = (op: types.Operation) => Promise<void>;

/**
 * Show a context menu for the given entry
 */
export async function showContextMenu(
  entry: types.TreeEntry,
  type: types.EntryType,
  status: EntryStatus,
  operate: OperateCb,
): Promise<void> {
  if (type === 'inconsistent') {
    console.error('inconsistent entry type');
    return;
  }

  const menu = await Menu.new();

  const hasLocal = 'local' in entry.entry || 'sync' in entry.entry;

  if (hasLocal) {
    menu.append(
      await MenuItem.new({
        text: 'Open',
        action: async () => openPath(entry.path),
      })
    );
  }

  if (status !== 'syncFull') {
    const text = type == 'directory' ? 'Synchronize all' : 'Synchronize';
    const op: SyncOp = type === 'directory' ? 'syncDeep' : 'sync';
    menu.append(await syncItem(operate, text, entry.path, op));
  }

  if (status === 'conflict' || status === 'conflictFull') {
    const text = type === 'directory' ? 'Resolve All Conflicts' : 'Resolve Conflict';
    const op: ResolveOp = type === 'directory' ? 'resolveDeep' : 'resolve';
    const resolve_menu = await Submenu.new({ text });
    resolve_menu.append(await Promise.all([
      resolveItem(operate, 'Replace older by newer', entry.path, op, 'replaceOlderByNewer'),
      resolveItem(operate, 'Replace newer by older', entry.path, op, 'replaceNewerByOlder'),
      resolveItem(operate, 'Replace local by remote', entry.path, op, 'replaceLocalByRemote'),
      resolveItem(operate, 'Replace remote by local', entry.path, op, 'replaceRemoteByLocal'),
      resolveItem(operate, 'Delete older', entry.path, op, 'deleteOlder'),
      resolveItem(operate, 'Delete newer', entry.path, op, 'deleteNewer'),
      resolveItem(operate, 'Delete local', entry.path, op, 'deleteLocal'),
      resolveItem(operate, 'Delete remote', entry.path, op, 'deleteRemote')
    ]));
    menu.append(resolve_menu);
  }

  menu.popup();
}

type SyncOp = 'sync' | 'syncDeep';

async function syncItem(operate: OperateCb, text: string, path: string, op: SyncOp) {
  const action =
    op == 'sync'
      ? async () => {
          operate({
            sync: path
          });
        }
      : async () => {
          operate({
            syncDeep: path
          });
        };
  return await MenuItem.new({
    text,
    action
  });
}

type ResolveOp = 'resolve' | 'resolveDeep';

async function resolveItem(
  operate: OperateCb,
  text: string,
  path: string,
  op: ResolveOp,
  method: types.ResolutionMethod, 
) {
  const action =
    op == 'resolve'
      ? async () => {
          operate({
            resolve: [path, method]
          });
        }
      : async () => {
          operate({
            resolveDeep: [path, method]
          });
        };

  return await MenuItem.new({
    text,
    action
  });
}
