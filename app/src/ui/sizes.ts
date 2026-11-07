// Splits the user drags are a preference: kept across launches (Storage),
// clamped by each panel's own minimum and maximum when they come back.

import { signal, Storage } from "nsh";

const KEY = "sonde.sizes";
const sizes = signal<Record<string, number>>({});
let timer: ReturnType<typeof setTimeout> | undefined;

void Storage.getItem(KEY)
  .then((v) => v && sizes.set({ ...JSON.parse(v), ...sizes() }))
  .catch(() => {});

/** A remembered size and its setter, for a ResizablePanel's `size` and `onResize`. */
export function rememberedSize(name: string, initial: number) {
  return {
    size: () => sizes()[name] ?? initial,
    onResize: (e: { size: number }) => {
      if (Math.round(e.size) === Math.round(sizes()[name] ?? -1)) return;
      sizes.set({ ...sizes(), [name]: Math.round(e.size) });
      clearTimeout(timer);
      timer = setTimeout(() => void Storage.setItem(KEY, JSON.stringify(sizes())).catch(() => {}), 400);
    },
  };
}
