import { goto } from '$app/navigation';
import { instanceGetAll } from '$lib/model';
import type types from '$lib/types';

export const prerender = true
export const ssr = false

export async function load(): Promise<{instances: types.Instance[]}> {
  const instances = await instanceGetAll();
  if (instances.length === 0) {
    await goto('/new');
  }
  return {
    instances
  };
}
