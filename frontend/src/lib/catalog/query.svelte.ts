/**
 * Reload a client-side query when its reactive inputs change. Dropping the
 * previous response prevents a slow request from replacing the active page.
 */
export function catalogQuery<T>(load: (signal: AbortSignal) => Promise<T> | null) {
  let revision = $state(0);
  let data = $state.raw<T | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let status = $state<number>();

  $effect(() => {
    void revision;
    const controller = new AbortController();
    const request = load(controller.signal);
    let current = true;
    data = null;
    failed = false;
    status = undefined;
    loading = request !== null;
    request?.then(
      result => { if (current) data = result; },
      (error) => {
        if (!current) return;
        status = typeof error?.status === "number" ? error.status : undefined;
        failed = true;
      },
    ).finally(() => { if (current) loading = false; });

    return () => { current = false; controller.abort(); };
  });

  return {
    get data() { return data; },
    get loading() { return loading; },
    get failed() { return failed; },
    get status() { return status; },
    retry() { revision++; },
  };
}

/** Queries of one collection refresh together after a successful mutation. */
export function createCatalogCollection() {
  let revision = $state(0);
  return {
    query<T>(load: () => Promise<T> | null) {
      return catalogQuery(() => {
        void revision;
        return load();
      });
    },
    invalidate() { revision++; },
  };
}
