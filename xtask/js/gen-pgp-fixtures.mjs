import { writeFileSync } from 'node:fs';
const openpgp = await import(
	new URL('../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const variant = process.argv[2] ?? 'v4';
if (variant !== 'v4' && variant !== 'v6') {
	throw new Error(`unknown variant ${variant}, expected v4 or v6`);
}
const suffix = variant === 'v6' ? '-v6' : '';

const dir = new URL('../../fixtures/', import.meta.url);
const passphrase = 'YnJlYWQtYW5kLXNhbHQtZml4dHVyZS1wYXNzcGhyYXNlLTAx';
const userIDs = [{ name: 'Fixture Account', email: 'fixture@thelemail.local' }];

async function generate() {
	if (variant === 'v4') {
		const { privateKey, publicKey } = await openpgp.generateKey({
			type: 'curve25519',
			userIDs,
			format: 'object'
		});
		return { privateKey, publicKey, locked: await openpgp.encryptKey({ privateKey, passphrase }) };
	}
	const keys = await import(
		new URL('../../web-client/packages/core/src/keys/pgpKeys.ts', import.meta.url).href
	);
	const { privateKey, publicKey } = await keys.generateCurve25519Key({ userIDs, date: new Date() });
	return { privateKey, publicKey, locked: await keys.lockKey(privateKey, passphrase) };
}

const { privateKey, publicKey, locked } = await generate();

writeFileSync(new URL(`keys/account${suffix}.pub.asc`, dir), publicKey.armor());
writeFileSync(new URL(`keys/account${suffix}.enc.asc`, dir), locked.armor());

const plaintexts = {
	'body-plain': 'Subject: fixture\r\n\r\nplain body for interop\r\n',
	'body-mime-multipart':
		'Content-Type: multipart/alternative; boundary="b1"\r\n\r\n' +
		'--b1\r\nContent-Type: text/plain\r\n\r\nhello plain\r\n' +
		'--b1\r\nContent-Type: text/html\r\n\r\n<p>hello html</p>\r\n--b1--\r\n',
	preview: JSON.stringify({
		v: 1,
		subject: 'Fixture subject',
		sender: { display: 'Fixture', address: 'fixture@thelemail.local' },
		recipients: [{ display: 'Rec', address: 'rec@thelemail.local', kind: 'to' }],
		snippet: 'plain body for interop',
		display_date: '2026-08-31T12:00:00Z'
	})
};

const meta = { passphrase, fingerprint: privateKey.getFingerprint(), messages: {} };

for (const [name, text] of Object.entries(plaintexts)) {
	const message = await openpgp.createMessage({ text });
	const armored = await openpgp.encrypt({
		message,
		encryptionKeys: publicKey,
		format: 'binary'
	});
	writeFileSync(new URL(`messages/${name}${suffix}.js.pgp`, dir), Buffer.from(armored));
	meta.messages[name] = { plaintext: text, producer: 'openpgp.js' };
}

writeFileSync(new URL(`keys/meta${suffix}.json`, dir), JSON.stringify(meta, null, '\t') + '\n');
console.log('fingerprint', meta.fingerprint);
