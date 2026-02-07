import { describe, it, expect } from 'vitest';
import en from '$lib/i18n/translations/en';
import fr from '$lib/i18n/translations/fr';
import es from '$lib/i18n/translations/es';
import zh from '$lib/i18n/translations/zh';
import ru from '$lib/i18n/translations/ru';

type Dict = Record<string, string>;
const translations: Record<string, Dict> = {
	en: en as unknown as Dict,
	fr: fr as unknown as Dict,
	es: es as unknown as Dict,
	zh: zh as unknown as Dict,
	ru: ru as unknown as Dict,
};
const locales = Object.keys(translations);
const enDict = translations['en'];

describe('i18n translations', () => {
	it('all locales have translations', () => {
		expect(locales).toHaveLength(5);
	});

	it('English has all required common keys', () => {
		expect(enDict['common.cancel']).toBe('Cancel');
		expect(enDict['common.save']).toBe('Save');
		expect(enDict['common.delete']).toBe('Delete');
		expect(enDict['common.loading']).toBeDefined();
	});

	it('French has all required common keys', () => {
		const frDict = translations['fr'];
		expect(frDict['common.cancel']).toBe('Annuler');
		expect(frDict['common.save']).toBe('Enregistrer');
	});

	it('all locales have the same number of keys as English', () => {
		const enKeys = Object.keys(enDict);
		for (const locale of locales) {
			if (locale === 'en') continue;
			const localeKeys = Object.keys(translations[locale]);
			const missingKeys = enKeys.filter(k => !localeKeys.includes(k));
			expect(missingKeys).toEqual([]);
		}
	});

	it('no translation has empty string values', () => {
		for (const locale of locales) {
			const dict = translations[locale];
			for (const [key, value] of Object.entries(dict)) {
				expect(value.trim().length, `${locale}.${key} is empty`).toBeGreaterThan(0);
			}
		}
	});

	it('navigation keys exist in all locales', () => {
		const navKeys = ['nav.home', 'nav.explore', 'nav.library', 'nav.playlists', 'nav.favorites'];
		for (const locale of locales) {
			const dict = translations[locale];
			for (const key of navKeys) {
				expect(dict[key], `${locale} missing ${key}`).toBeDefined();
			}
		}
	});

	it('auth keys exist in all locales', () => {
		const authKeys = ['auth.login', 'nav.logout', 'auth.register'];
		for (const locale of locales) {
			const dict = translations[locale];
			for (const key of authKeys) {
				expect(dict[key], `${locale} missing ${key}`).toBeDefined();
			}
		}
	});

	it('parameter placeholders are preserved across locales', () => {
		const enKeysWithParams = Object.entries(enDict)
			.filter(([_, v]) => v.includes('{'))
			.map(([k]) => k);

		for (const key of enKeysWithParams) {
			const enParams = [...enDict[key].matchAll(/\{(\w+)\}/g)].map(m => m[1]).sort();
			for (const locale of locales) {
				if (locale === 'en') continue;
				const val = translations[locale][key];
				if (!val) continue;
				const localeParams = [...val.matchAll(/\{(\w+)\}/g)].map(m => m[1]).sort();
				expect(localeParams, `${locale}.${key} params mismatch`).toEqual(enParams);
			}
		}
	});
});
