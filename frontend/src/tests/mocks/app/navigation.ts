import { page } from "./state";
export function goto(url: string | URL) {
	page.url = new URL(url, page.url.href);
	return Promise.resolve();
}

export function invalidate(url: string) {
	return Promise.resolve();
}

export function invalidateAll() {
	return Promise.resolve();
}
