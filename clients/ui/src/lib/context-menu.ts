import { Menu, MenuItem, Submenu } from '@tauri-apps/api/menu';
import type types from './types';
import type { EntryStatus } from './model';

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

  if (status === 'conflict' || status === 'conflictFull') {
    const resolve = await Submenu.new({ text: 'Resolve' });
    resolve.append([
      await MenuItem.new({
        text: 'Replace older by newer',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Replace newer by older',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Replace local by remote',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Replace remote by local',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Delete older',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Delete newer',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Delete local',
        action: async () => {}
      }),
      await MenuItem.new({
        text: 'Delete remote',
        action: async () => {}
      })
    ]);
    menu.append(resolve);
  }

  menu.popup();
}
