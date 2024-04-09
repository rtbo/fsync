import { daemonConnect, daemonNodeAndChildren } from '$lib/ipc';
import type types from '$lib/types';

export const prerender = true;
export const ssr = false;

export async function load({params}): Promise<types.NodeAndChildren> {
    await daemonConnect(params.instanceName);
    const res = await daemonNodeAndChildren('/');
    console.log('initial fetch of node and children');
    return res;
}
