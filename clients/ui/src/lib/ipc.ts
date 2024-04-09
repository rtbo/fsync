import { invoke } from "@tauri-apps/api/tauri";
import type types from "./types";

export async function errorMessage(err: types.Error): Promise<string> {
  return await invoke('error_message', {
    err
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
  return invoke('instance_create', args);
}

export async function daemonConnected(): Promise<boolean> {
  return invoke('daemon_connected');
}

export async function daemonInstanceName(): Promise<string> {
  return invoke('daemon_instance_name');
}

export async function daemonConnect(name?: string): Promise<void> {
  return invoke('daemon_connect', {
    name: name ?? null
  });
}

export async function daemonNodeAndChildren(path: string | null): Promise<types.NodeAndChildren> {
  return invoke('daemon_node_and_children', { path });
}

export async function daemonOperate(operation: types.Operation): Promise<types.Progress> {
  return invoke('daemon_operate', {
    operation
  });
}

export async function daemonProgress(path: string): Promise<types.Progress | null> {
  return invoke('daemon_progress', {
    path
  });
}

export async function daemonProgresses(path: string): Promise<types.PathProgress[]> {
  return invoke('daemon_progresses', {
    path
  });
}