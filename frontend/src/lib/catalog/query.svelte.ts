/**
 * Reload a client-side query when its reactive inputs change. Dropping the
 * previous response prevents a slow request from replacing the active page.
 */
export function catalogQuery<T>(load: (signal: AbortSignal) => Promise<T> | null, timeoutMs?: number) {
  let revision = $state(0);
  let data = $state.raw<T | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let settledOnce = $state(false);
  let status = $state<number>();

  $effect(() => {
    void revision;
    const controller = new AbortController();
    const request = load(controller.signal);
    const timeout = request && timeoutMs ? setTimeout(() => controller.abort(), timeoutMs) : undefined;
    let current = true;
    data = null;
    failed = false;
    status = undefined;
    loading = request !== null;
    if (!request) settledOnce = true;
    request?.then(
      result => { if (current) data = result; },
      (error) => {
        if (!current) return;
        status = typeof error?.status === "number" ? error.status : undefined;
        failed = true;
      },
    ).finally(() => {
      clearTimeout(timeout);
      if (current) {
        loading = false;
        settledOnce = true;
      }
    });

    return () => { current = false; clearTimeout(timeout); controller.abort(); };
  });

  return {
    get data() { return data; },
    get loading() { return loading; },
    get initialLoading() { return !settledOnce; },
    get failed() { return failed; },
    get status() { return status; },
    retry() { revision++; },
  };
}

/** A stalled shelf must not hold back the other sections on the same page. */
export function catalogSection<T>(load: (signal: AbortSignal) => Promise<T> | null) {
  return catalogQuery(load, 15_000);
}

/** Queries of one collection refresh together after a successful mutation. */
export function createCatalogCollection() {
  let revision = $state(0);
  return {
    query<T>(load: (signal: AbortSignal) => Promise<T> | null) {
      return catalogQuery(signal => {
        void revision;
        return load(signal);
      });
    },
    invalidate() { revision++; },
  };
}
