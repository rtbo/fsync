import type types from './types';
import { daemonProgresses } from './model';

type Subscriber = (progress: types.PathProgress[]) => void;
type Cb = () => void;

export function createProgressesStore(path: string, globDoneCb?: Cb) {
  const subscribers = new Set<Subscriber>();
  let progresses: types.PathProgress[] = [];
  let interval: number | undefined = undefined;
  const doneCb: Map<string, () => void> = new Map();

  function set(progress: types.PathProgress[]) {
    progresses = progress;
    subscribers.forEach((fn) => fn(progress));
  }

  function checkDone(newProgresses: types.PathProgress[]) {
    let pathes = progresses.map((p) => p.path);
    newProgresses.forEach((pp) => {
      pathes = pathes.filter((path) => path !== pp.path);
    });
    if (pathes.length && globDoneCb !== undefined) {
      globDoneCb();
    }
    pathes.forEach((p) => {
      const cb = doneCb.get(p);
      if (cb !== undefined) {
        cb();
        doneCb.delete(p);
      }
    });
  }

  function stopListen() {
    if (interval !== undefined) {
      console.debug('stopListen');
      clearInterval(interval);
      interval = undefined;
    }
  }

  function startListen() {
    if (interval === undefined) {
      interval = setInterval(async () => {
        const newProgresses: types.PathProgress[] = await daemonProgresses(path);
        if (newProgresses.length === 0) {
          stopListen();
        }
        checkDone(newProgresses);
        set(newProgresses);
      }, 200);
    }
  }

  function subscribe(subscriber: Subscriber): () => void {
    subscribers.add(subscriber);
    subscriber(progresses);
    return () => {
      subscribers.delete(subscriber);
      if (subscribers.size === 0) {
        stopListen();
      }
    };
  }

  function add(progress: types.PathProgress, cb?: Cb) {
    let found = false;
    progresses.forEach((p) => {
      if (p.path === progress.path) {
        found = true;
        p.progress = progress.progress;
      }
    });
    if (!found) {
      progresses = [...progresses, progress];
    }
    if (cb !== undefined) {
      doneCb.set(progress.path, cb);
    }
    set(progresses);
    startListen();
  }

  function remove(path: string) {
    progresses = progresses.filter((p) => p.path !== path);
    const cb = doneCb.get(path);
    if (cb !== undefined) {
      cb();
      doneCb.delete(path);
    }
    set(progresses);
  }

  async function check() {
    const progress: types.PathProgress[] = await daemonProgresses(path);
    set(progress);
    if (progress.length > 0) {
      startListen();
    } else {
      stopListen();
    }
  }

  check();

  return {
    subscribe,
    check,
    add,
    remove,
  };
}
