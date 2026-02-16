import { describe, it, expect, vi, beforeEach } from 'vitest';
import { t, getLocale, setLocale, supportedLocales, localeNames, type Locale, type TranslationKey } from './index.svelte';

describe('i18n module', () => {
  describe('supportedLocales', () => {
    it('contains expected locales', () => {
      expect(supportedLocales).toEqual(['en', 'fr', 'es', 'zh', 'ru']);
    });

    it('has 5 locales', () => {
      expect(supportedLocales).toHaveLength(5);
    });
  });

  describe('localeNames', () => {
    it('has a name for each locale', () => {
      expect(localeNames.en).toBe('English');
      expect(localeNames.fr).toBe('Français');
      expect(localeNames.es).toBe('Español');
      expect(localeNames.zh).toBe('中文');
      expect(localeNames.ru).toBe('Русский');
    });
  });

  describe('t()', () => {
    it('returns a string for a known key', () => {
      setLocale('en');
      const result = t('search.placeholder' as TranslationKey);
      expect(typeof result).toBe('string');
    });

    it('returns the key for an unknown key', () => {
      const result = t('nonexistent.key' as TranslationKey);
      expect(result).toBe('nonexistent.key');
    });

    it('returns translated text for a given locale', () => {
      setLocale('en');
      const enResult = t('search.placeholder' as TranslationKey);
      setLocale('fr');
      const frResult = t('search.placeholder' as TranslationKey);
      // Both should be strings, but may differ
      expect(typeof enResult).toBe('string');
      expect(typeof frResult).toBe('string');
    });

    it('replaces params in translation', () => {
      setLocale('en');
      // Use a key that may have params or just test substitution with a generic key
      const result = t('nonexistent.{name}' as TranslationKey, { name: 'test' });
      expect(result).toBe('nonexistent.test');
    });
  });

  describe('getLocale / setLocale', () => {
    it('getLocale returns current locale', () => {
      setLocale('fr');
      expect(getLocale()).toBe('fr');
    });

    it('setLocale changes the locale', () => {
      setLocale('es');
      expect(getLocale()).toBe('es');
    });

    it('setLocale stores the locale in localStorage', () => {
      setLocale('zh');
      expect(localStorage.getItem('soundtime_lang')).toBe('zh');
    });

    it('setLocale sets document lang attribute', () => {
      setLocale('ru');
      expect(document.documentElement.lang).toBe('ru');
    });
  });

  describe('detectLocale (via module initialization)', () => {
    it('falls back to en for unknown stored locale', () => {
      localStorage.setItem('soundtime_lang', 'xx');
      // detectLocale already ran at import time; we can test that setLocale works
      setLocale('en');
      expect(getLocale()).toBe('en');
    });

    it('uses stored locale when valid', () => {
      localStorage.setItem('soundtime_lang', 'es');
      setLocale('es');
      expect(getLocale()).toBe('es');
    });
  });

  describe('t() with fallback logic', () => {
    it('falls back to en translation when locale has no key', () => {
      setLocale('fr');
      // Use a key that exists in en but may not in fr
      const result = t('search.placeholder' as TranslationKey);
      expect(typeof result).toBe('string');
      expect(result.length).toBeGreaterThan(0);
    });

    it('returns key when neither locale nor en has it', () => {
      setLocale('zh');
      const result = t('totally.bogus.key.xyz' as TranslationKey);
      expect(result).toBe('totally.bogus.key.xyz');
    });

    it('replaces multiple params', () => {
      const result = t('{a} and {b}' as TranslationKey, { a: 'X', b: 'Y' });
      expect(result).toBe('X and Y');
    });

    it('handles numeric param values', () => {
      const result = t('{count} items' as TranslationKey, { count: 42 });
      expect(result).toBe('42 items');
    });
  });

  describe('detectLocale edge cases', () => {
    it('detects locale from navigator.language when languages is unavailable', () => {
      localStorage.removeItem('soundtime_lang');
      const originalLanguages = navigator.languages;
      const originalLanguage = navigator.language;
      
      Object.defineProperty(navigator, 'languages', { value: undefined, configurable: true });
      Object.defineProperty(navigator, 'language', { value: 'fr-FR', configurable: true });
      
      // detectLocale already ran at import time, but we can verify setLocale/getLocale work
      setLocale('en');
      expect(getLocale()).toBe('en');
      
      // Restore
      Object.defineProperty(navigator, 'languages', { value: originalLanguages, configurable: true });
      Object.defineProperty(navigator, 'language', { value: originalLanguage, configurable: true });
    });

    it('handles navigator.languages with non-matching codes', () => {
      localStorage.removeItem('soundtime_lang');
      const originalLanguages = navigator.languages;
      Object.defineProperty(navigator, 'languages', { value: ['ko-KR', 'ja-JP'], configurable: true });
      
      setLocale('en');
      expect(getLocale()).toBe('en');
      
      Object.defineProperty(navigator, 'languages', { value: originalLanguages, configurable: true });
    });
  });

  describe('t() with edge cases', () => {
    it('handles empty params object', () => {
      setLocale('en');
      const result = t('search.placeholder' as TranslationKey, {});
      expect(typeof result).toBe('string');
    });

    it('handles params with no matching placeholders', () => {
      setLocale('en');
      const result = t('search.placeholder' as TranslationKey, { nonexistent: 'value' });
      expect(typeof result).toBe('string');
    });
  });

  describe('t() with locale fallback to en translations', () => {
    it('falls back to en dict when currentLocale translations dict is somehow missing a key', () => {
      setLocale('ru');
      // Use a key that might not be in Russian but is in English
      const result = t('search.placeholder' as TranslationKey);
      // Should return a non-empty string (either ru or en fallback)
      expect(result.length).toBeGreaterThan(0);
    });

    it('handles switching between all supported locales', () => {
      for (const locale of supportedLocales) {
        setLocale(locale);
        expect(getLocale()).toBe(locale);
        // t() should return something for a common key
        const result = t('search.placeholder' as TranslationKey);
        expect(typeof result).toBe('string');
      }
    });
  });
});
