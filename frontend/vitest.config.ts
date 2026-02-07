import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
	plugins: [svelte({ hot: false })],
	test: {
		globals: true,
		environment: 'jsdom',
		include: ['src/**/*.{test,spec}.{js,ts}'],
		setupFiles: ['src/tests/setup.ts'],
		coverage: {
			provider: 'v8',
			reporter: ['text', 'json', 'html'],
			include: ['src/lib/**/*.{ts,svelte}'],
			exclude: [
				'src/lib/i18n/translations/**',
				'src/lib/types.ts',
				'node_modules/**'
			],
			thresholds: {
				lines: 75,
				branches: 75,
				functions: 75,
				statements: 75
			}
		},
		alias: {
			$lib: '/src/lib',
			$app: '/src/tests/mocks/app'
		}
	}
});
