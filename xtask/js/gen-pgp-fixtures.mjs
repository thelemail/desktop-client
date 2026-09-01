import { writeFileSync } from 'node:fs';
const openpgp = await import(
	new URL('../../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const dir = new URL('../../fixtures/', import.meta.url);
const passphrase = 'YnJlYWQtYW5kLXNhbHQtZml4dHVyZS1wYXNzcGhyYXNlLTAx';

const { privateKey, publicKey } = await openpgp.generateKey({
	type: 'curve25519',
	userIDs: [{ name: 'Fixture Account', email: 'fixture@thelemail.local' }],
	format: 'object'
});

const locked = await openpgp.encryptKey({ privateKey, passphrase });

writeFileSync(new URL('keys/account.pub.asc', dir), publicKey.armor());
writeFileSync(new URL('keys/account.enc.asc', dir), locked.armor());

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
	writeFileSync(new URL(`messages/${name}.js.pgp`, dir), Buffer.from(armored));
	meta.messages[name] = { plaintext: text, producer: 'openpgp.js' };
}

writeFileSync(new URL('keys/meta.json', dir), JSON.stringify(meta, null, '\t') + '\n');
console.log('fingerprint', meta.fingerprint);
