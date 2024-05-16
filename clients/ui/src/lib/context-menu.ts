import { Menu, MenuItem, Submenu } from '@tauri-apps/api/menu';
import type types from './types';
import type { EntryStatus } from './model';
import { daemonOperate } from './ipc';

/**
 * Show a context menu for the given entry
 */
export async function showContextMenu(
  entry: types.TreeEntry,
  type: types.EntryType,
  status: EntryStatus
): Promise<void> {
  if (type === 'inconsistent') {
    console.error('inconsistent entry type');
    return;
  }

  const menu = await Menu.new();

  menu.append(
    await MenuItem.new({
      text: 'Open',
      action: async () => {}
    })
  );

  if (status !== 'syncFull') {
    const text = type == 'directory' ? 'Synchronize all' : 'Synchronize';
    const op: SyncOp = type === 'directory' ? 'syncDeep' : 'sync';
    menu.append(await syncItem(text, entry.path, op));
  }

  if (status === 'conflict' || status === 'conflictFull') {
    const text = type === 'directory' ? 'Resolve All Conflicts' : 'Resolve Conflict';
    const op: ResolveOp = type === 'directory' ? 'resolveDeep' : 'resolve';
    const resolve_menu = await Submenu.new({ text });
    resolve_menu.append(await Promise.all([
      resolveItem('Replace older by newer', entry.path, op, 'replaceOlderByNewer'),
      resolveItem('Replace newer by older', entry.path, op, 'replaceNewerByOlder'),
      resolveItem('Replace local by remote', entry.path, op, 'replaceLocalByRemote'),
      resolveItem('Replace remote by local', entry.path, op, 'replaceRemoteByLocal'),
      resolveItem('Delete older', entry.path, op, 'deleteOlder'),
      resolveItem('Delete newer', entry.path, op, 'deleteNewer'),
      resolveItem('Delete local', entry.path, op, 'deleteLocal'),
      resolveItem('Delete remote', entry.path, op, 'deleteRemote')
    ]));
    menu.append(resolve_menu);
  }

  menu.popup();
}

type SyncOp = 'sync' | 'syncDeep';

async function syncItem(text: string, path: string, op: SyncOp) {
  const action =
    op == 'sync'
      ? async () => {
          daemonOperate({
            sync: path
          });
        }
      : async () => {
          daemonOperate({
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
  text: string,
  path: string,
  op: ResolveOp,
  method: types.ResolutionMethod
) {
  const action =
    op == 'resolve'
      ? async () => {
          daemonOperate({
            resolve: [path, method]
          });
        }
      : async () => {
          daemonOperate({
            resolveDeep: [path, method]
          });
        };

  return await MenuItem.new({
    text,
    action
  });
}
