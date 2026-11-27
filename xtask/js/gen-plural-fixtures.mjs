import { writeFileSync } from 'node:fs';

const locales = ['en', 'de', 'fr', 'pt'];
const numbers = [...Array.from({ length: 1201 }, (_, i) => i), 1000000, 1000001, 2000000, 1000000000];

const categories = {};
for (const locale of locales) {
	categories[locale] = {};
	for (const type of ['cardinal', 'ordinal']) {
		const rules = new Intl.PluralRules(locale, { type });
		const byCategory = {};
		for (const n of numbers) {
			const category = rules.select(n);
			if (category === 'other') continue;
			(byCategory[category] ??= []).push(n);
		}
		categories[locale][type] = byCategory;
	}
}

const out = new URL('../../fixtures/i18n/plural.json', import.meta.url);
writeFileSync(out, JSON.stringify({ icu: process.versions.icu, numbers, categories }) + '\n');
console.log(`wrote ${numbers.length} numbers for ${locales.length} locales`);
