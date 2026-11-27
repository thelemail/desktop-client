import { mkdirSync, writeFileSync } from 'node:fs';
import { register } from 'node:module';

const core = new URL('../../web-client/packages/core/src/', import.meta.url).href;
const hooks = `
export async function resolve(specifier, context, next) {
	if (specifier === '$paraglide/messages.js') {
		return { url: 'data:text/javascript,export const m = { mailbox_unknown_sender: () => "Unknown" };', shortCircuit: true };
	}
	if (specifier.startsWith('$core/')) {
		return next(${JSON.stringify(core)} + specifier.slice(6) + '.ts', context);
	}
	return next(specifier, context);
}`;
register('data:text/javascript,' + encodeURIComponent(hooks));

const { initialsFor } = await import('../../web-client/packages/core/src/mail/initials.ts');
const { paletteFor } = await import('../../web-client/packages/core/src/mail/avatarPalette.ts');
const { attestedMismatch, senderFields } = await import(
	'../../web-client/packages/core/src/mail/attestedSender.ts'
);

const dir = new URL('../../fixtures/avatar/', import.meta.url);
mkdirSync(dir, { recursive: true });

const palette = [
	'var(--brass-100)',
	'var(--pine-100)',
	'var(--info-100)',
	'var(--success-100)',
	'var(--bg-raised)'
];
const slot = (seed) => palette.indexOf(paletteFor(seed).bg);

const names = [
	['Ada Lovelace', 'ada@acme.test'],
	['ada', 'ada@acme.test'],
	['Mary Ann Lee', 'mary@acme.test'],
	['  Élodie   Durand ', 'elodie@acme.test'],
	['Ølstad\tKari', 'kari@acme.test'],
	['李小龙', 'li@acme.test'],
	['o', 'o@acme.test'],
	['', 'zoe.west@acme.test'],
	[null, 'zoe.west@acme.test'],
	['   ', 'q@acme.test'],
	['', ''],
	[null, null],
	['', '@acme.test'],
	['straße', 'x@acme.test'],
	['Jean Paul', 'jp@acme.test'],
	['﻿Bom Name', 'b@acme.test'],
	['Ünal Öz', 'unal@acme.test']
];

const seeds = [
	'',
	'a',
	'ada@acme.test',
	'zoe.west@acme.test',
	'postmaster@thelemail.com',
	'élodie@acme.test',
	'李@acme.test',
	'x'.repeat(200) + '@acme.test',
	'emoji😀@acme.test'
];

const senders = [
	[{ display: 'Ada Lovelace', address: 'ada@acme.test' }, null],
	[{ display: 'Ada Lovelace', address: 'ada@acme.test' }, ''],
	[{ display: 'Ada Lovelace', address: 'ada@acme.test' }, 'ada@acme.test'],
	[{ display: 'Ada Lovelace', address: 'Ada@Acme.Test ' }, ' ada@acme.test'],
	[{ display: 'Bank Support', address: 'support@bank.test' }, 'mallory@thelemail.com'],
	[{ display: '', address: 'ops@acme.test' }, null],
	[{ display: '', address: '' }, null],
	[{ display: 'Élodie', address: 'élodie@acme.test' }, 'élodie@acme.test'],
	[{ display: 'Zoe', address: 'zoe@acme.test' }, 'ZOE@ACME.TEST']
];

const cases = {
	initials: names.map(([name, email]) => ({ name, email, initials: initialsFor(name, email) })),
	palette: seeds.map((seed) => ({ seed, slot: slot(seed) })),
	senders: senders.map(([sender, attested]) => {
		const fields = senderFields(sender, attested);
		return {
			sender,
			attested,
			mismatch: attestedMismatch(attested, sender.address),
			from: fields.from,
			fromAddr: fields.fromAddr,
			init: fields.init,
			slot: palette.indexOf(fields.bg),
			senderMismatch: fields.senderMismatch === true
		};
	})
};

writeFileSync(new URL('cases.json', dir), JSON.stringify(cases, null, '\t') + '\n');
console.log(
	`wrote ${cases.initials.length} initials, ${cases.palette.length} palette and ${cases.senders.length} sender cases`
);
