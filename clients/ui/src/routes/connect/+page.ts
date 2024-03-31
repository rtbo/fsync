import { instanceGetAll } from '$lib/api';
import type types from '$lib/types';

export async function load(): Promise<{instances: types.Instance[]}> {
  const instances = await instanceGetAll();
  return {
    instances
  };
}
