import { describe, it, expect } from 'vitest';
import { formatDuration, formatDate, cn } from '$lib/utils';

describe('formatDuration', () => {
	it('formats 0 seconds', () => {
		expect(formatDuration(0)).toBe('0:00');
	});

	it('formats seconds < 60', () => {
		expect(formatDuration(45)).toBe('0:45');
	});

	it('formats 1 minute exactly', () => {
		expect(formatDuration(60)).toBe('1:00');
	});

	it('formats minutes and seconds', () => {
		expect(formatDuration(195)).toBe('3:15');
	});

	it('formats long duration', () => {
		expect(formatDuration(3661)).toBe('61:01');
	});

	it('pads single-digit seconds with zero', () => {
		expect(formatDuration(65)).toBe('1:05');
	});

	it('handles fractional seconds', () => {
		expect(formatDuration(90.7)).toBe('1:30');
	});
});

describe('formatDate', () => {
	it('formats a date string', () => {
		const result = formatDate('2024-06-15T10:30:00Z');
		// The format depends on locale (fr-FR), but should contain year, month, day
		expect(result).toContain('2024');
		expect(result).toContain('15');
	});

	it('formats ISO date string', () => {
		const result = formatDate('2023-01-01');
		expect(result).toContain('2023');
	});
});

describe('cn', () => {
	it('merges class names', () => {
		const result = cn('px-2', 'py-1');
		expect(result).toContain('px-2');
		expect(result).toContain('py-1');
	});

	it('resolves conflicts with tailwind-merge', () => {
		const result = cn('px-2', 'px-4');
		expect(result).toBe('px-4');
	});

	it('handles conditional classes', () => {
		const result = cn('base', false && 'hidden', 'extra');
		expect(result).toContain('base');
		expect(result).toContain('extra');
		expect(result).not.toContain('hidden');
	});

	it('handles undefined and null', () => {
		const result = cn('base', undefined, null, 'extra');
		expect(result).toContain('base');
		expect(result).toContain('extra');
	});

	it('handles empty input', () => {
		const result = cn();
		expect(result).toBe('');
	});
});
