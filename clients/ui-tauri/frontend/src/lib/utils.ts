import type { SelectOptionType } from 'flowbite-svelte';

export function selectName<T>(value: T, options: SelectOptionType<T>[]): string | number | null {
  return options.find((opt) => opt.value === value)?.name ?? null;
}

export function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function debounce<T>(ms: number, handler: (val: T) => void) {
  const obj: { timer: number | undefined } = {
    timer: undefined
  };
  return function (val: T) {
    clearTimeout(obj.timer);
    obj.timer = setTimeout(() => handler(val), ms);
  };
}
