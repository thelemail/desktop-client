import { writeFileSync } from 'node:fs';
import {
	wrapMasterKey,
	unwrapMasterKey,
	deriveMasterKeyId,
	derivePgpPassphrase
} from '../../web-client/src/lib/keystore/opaque-params.ts';

const hex = (b) => Buffer.from(b).toString('hex');
const vectors = [];

for (let i = 0; i < 6; i++) {
	const exportKey = new Uint8Array(32).map((_, j) => (j * 13 + i * 41) & 0xff);
	const amk = new Uint8Array(32).map((_, j) => (j * 7 + i * 31) & 0xff);
	for (const recovery of [false, true]) {
		const wrapped = await wrapMasterKey(exportKey, amk, recovery);
		const back = await unwrapMasterKey(exportKey, wrapped, recovery);
		if (hex(back) !== hex(amk)) throw new Error('js roundtrip failed');
		vectors.push({
			exportKey: hex(exportKey),
			amk: hex(amk),
			recovery,
			wrapped: hex(wrapped),
			masterKeyId: hex(await deriveMasterKeyId(amk)),
			pgpPassphrase: await derivePgpPassphrase(amk)
		});
	}
}

const out = new URL('../../fixtures/amk/vectors.json', import.meta.url);
writeFileSync(out, JSON.stringify({ source: 'web-client/src/lib/keystore/opaque-params.ts', vectors }, null, '\t') + '\n');
console.log(`wrote ${vectors.length} vectors`);
