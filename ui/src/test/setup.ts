import "@testing-library/react";

class MemoryStorage implements Storage {
  private m = new Map<string, string>();
  get length(): number {
    return this.m.size;
  }
  key(index: number): string | null {
    return [...this.m.keys()][index] ?? null;
  }
  getItem(key: string): string | null {
    return this.m.has(key) ? (this.m.get(key) as string) : null;
  }
  setItem(key: string, value: string): void {
    this.m.set(key, String(value));
  }
  removeItem(key: string): void {
    this.m.delete(key);
  }
  clear(): void {
    this.m.clear();
  }
}

const usable = (s: unknown): s is Storage =>
  !!s && typeof (s as Storage).clear === "function" && typeof (s as Storage).getItem === "function";

for (const name of ["localStorage", "sessionStorage"] as const) {
  let current: unknown;
  try {
    current = (globalThis as Record<string, unknown>)[name];
  } catch {
    current = undefined;
  }
  if (!usable(current)) {
    try {
      Object.defineProperty(globalThis, name, { value: new MemoryStorage(), configurable: true, writable: true });
    } catch {
      void 0;
    }
  }
}
