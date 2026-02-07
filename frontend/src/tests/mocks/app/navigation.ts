// Mock for $app/navigation
export function goto(url: string) {
	return Promise.resolve();
}

export function invalidate(url: string) {
	return Promise.resolve();
}

export function invalidateAll() {
	return Promise.resolve();
}
