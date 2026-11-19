import { fromStore, writable } from "svelte/store";

// Match SvelteKit's reactive URL so pagination and history-dependent views can be exercised.
const url = fromStore(writable(new URL("http://localhost/")));
export const page = {
  get url() { return url.current; },
  set url(value: URL) { url.current = value; },
  params: {} as Record<string, string>,
  route: { id: "/" },
  status: 200,
  error: null,
  data: {},
  form: null,
};
