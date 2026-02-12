import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';
import PluginSlot from './PluginSlot.svelte';

// Mock the pluginApi
vi.mock('$lib/api', () => ({
	pluginApi: {
		list: vi.fn(),
	},
}));

import { pluginApi } from '$lib/api';
import type { Plugin, PluginPermissions } from '$lib/types';

/** Helper to create a mock Plugin object with sensible defaults. */
function mockPlugin(overrides: Partial<Plugin> = {}): Plugin {
	return {
		id: 'p1',
		name: 'Test Plugin',
		version: '1.0.0',
		description: null,
		author: null,
		license: null,
		homepage: null,
		git_url: 'https://github.com/org/plugin.git',
		permissions: { events: [], http_hosts: [], write_tracks: false, config_access: false, read_users: false },
		status: 'enabled',
		error_message: null,
		installed_at: '2025-01-01T00:00:00Z',
		updated_at: '2025-01-01T00:00:00Z',
		...overrides,
	};
}

describe('PluginSlot', () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it('does not render when no plugins are loaded', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({ plugins: [] });
		const { container } = render(PluginSlot, { props: { slot: 'test-slot' } });
		await waitFor(() => {
			expect(container.querySelector('.plugin-slot')).toBeNull();
		});
	});

	it('renders plugin iframes for enabled plugins with permissions', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframe = container.querySelector('iframe');
			expect(iframe).toBeTruthy();
		});
	});

	it('sets sandbox="allow-scripts" on iframes (no allow-same-origin)', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframe = container.querySelector('iframe');
			expect(iframe?.getAttribute('sandbox')).toBe('allow-scripts');
			// SECURITY: must NOT contain 'allow-same-origin'
			expect(iframe?.getAttribute('sandbox')).not.toContain('allow-same-origin');
		});
	});

	it('sets correct src URL for plugin iframe', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'track-detail' } });
		await waitFor(() => {
			const iframe = container.querySelector('iframe');
			expect(iframe?.getAttribute('src')).toBe('/api/admin/plugins/p1/ui/track-detail');
		});
	});

	it('filters out disabled plugins', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [
				mockPlugin({ id: 'p1', name: 'Enabled', status: 'enabled' }),
				mockPlugin({ id: 'p2', name: 'Disabled', status: 'disabled' }),
			],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframes = container.querySelectorAll('iframe');
			expect(iframes.length).toBe(1);
		});
	});

	it('filters out plugins without permissions', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [
				mockPlugin({ id: 'p1', name: 'With Perms' }),
				mockPlugin({ id: 'p2', name: 'No Perms', permissions: null as unknown as PluginPermissions }),
			],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframes = container.querySelectorAll('iframe');
			expect(iframes.length).toBe(1);
		});
	});

	it('applies custom class to container', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar', class: 'my-custom-class' } });
		await waitFor(() => {
			const slot = container.querySelector('.plugin-slot');
			expect(slot?.classList.contains('my-custom-class')).toBe(true);
		});
	});

	it('uses slot name in container class', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'player-controls' } });
		await waitFor(() => {
			const slot = container.querySelector('.plugin-slot--player-controls');
			expect(slot).toBeTruthy();
		});
	});

	it('handles pluginApi.list() failure gracefully', async () => {
		vi.mocked(pluginApi.list).mockRejectedValue(new Error('Network error'));
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			expect(container.querySelector('.plugin-slot')).toBeNull();
		});
	});

	it('sets loading="lazy" on iframes', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin()],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframe = container.querySelector('iframe');
			expect(iframe?.getAttribute('loading')).toBe('lazy');
		});
	});

	it('sets descriptive title on iframes', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({
			plugins: [mockPlugin({ name: 'My Plugin' })],
		});
		const { container } = render(PluginSlot, { props: { slot: 'sidebar' } });
		await waitFor(() => {
			const iframe = container.querySelector('iframe');
			expect(iframe?.getAttribute('title')).toBe('My Plugin (sidebar)');
		});
	});

	it('handleMessage ignores non-object messages', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({ plugins: [] });
		render(PluginSlot, { props: { slot: 'sidebar' } });

		// Should not throw
		const dispatchSpy = vi.spyOn(window, 'dispatchEvent');
		window.dispatchEvent(new MessageEvent('message', { data: 'not an object' }));
		// No CustomEvent should be dispatched for plugin actions
		const pluginEvents = dispatchSpy.mock.calls.filter(
			(call) => call[0] instanceof CustomEvent && (call[0] as CustomEvent).type === 'soundtime:plugin-action'
		);
		expect(pluginEvents.length).toBe(0);
	});

	it('handleMessage ignores messages with wrong type', async () => {
		vi.mocked(pluginApi.list).mockResolvedValue({ plugins: [] });
		render(PluginSlot, { props: { slot: 'sidebar' } });

		const dispatchSpy = vi.spyOn(window, 'dispatchEvent');
		window.dispatchEvent(new MessageEvent('message', {
			data: { type: 'not-soundtime', action: 'test' }
		}));
		const pluginEvents = dispatchSpy.mock.calls.filter(
			(call) => call[0] instanceof CustomEvent && (call[0] as CustomEvent).type === 'soundtime:plugin-action'
		);
		expect(pluginEvents.length).toBe(0);
	});
});
