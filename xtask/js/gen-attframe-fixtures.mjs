import { writeFileSync } from 'node:fs';
import { build } from '../../../web-client/src/lib/mail/attframe.ts';

const dir = new URL('../../fixtures/attframe/', import.meta.url);
const cases = [
	{
		name: 'minimal',
		header: { filename: 'report.pdf', contentType: 'application/pdf', disposition: 'attachment' },
		payload: new Uint8Array([1, 2, 3, 4, 5])
	},
	{
		name: 'inline-cid',
		header: {
			filename: 'logo.png',
			contentType: 'image/png',
			disposition: 'inline',
			contentId: 'logo@thelemail'
		},
		payload: new Uint8Array(1024).fill(7)
	},
	{
		name: 'unicode-name',
		header: {
			filename: 'rapport été 😀.txt',
			contentType: 'text/plain; charset=utf-8',
			disposition: 'attachment'
		},
		payload: new TextEncoder().encode('contenu')
	}
];

const meta = [];
for (const c of cases) {
	const frame = build(c.header, c.payload);
	writeFileSync(new URL(`${c.name}.bin`, dir), Buffer.from(frame));
	meta.push({ ...c, payloadLen: c.payload.byteLength, payload: undefined });
}
writeFileSync(new URL('meta.json', dir), JSON.stringify(meta, null, '\t') + '\n');
console.log(`wrote ${cases.length} frames`);
