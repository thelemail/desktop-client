import { mkdirSync, writeFileSync } from 'node:fs';
import { isPgpEncryptedMime, extractPgpArmor } from '../../web-client/src/lib/mail/pgpMime.ts';

const dir = new URL('../../fixtures/mime/', import.meta.url);
mkdirSync(dir, { recursive: true });

const armor = '-----BEGIN PGP MESSAGE-----\nhQEMA1234\n=abcd\n-----END PGP MESSAGE-----';
const b64 = Buffer.from(armor, 'utf8').toString('base64').replace(/(.{64})/g, '$1\n');

const cases = {
	'plain-mime': 'Content-Type: text/plain\r\n\r\njust a plain body\r\n',
	'pgp-simple':
		'Content-Type: multipart/encrypted; protocol="application/pgp-encrypted"; boundary="b1"\r\n' +
		'\r\n--b1\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n' +
		`--b1\r\nContent-Type: application/octet-stream\r\n\r\n${armor}\r\n--b1--\r\n`,
	'pgp-base64':
		'Content-Type: multipart/encrypted; protocol="application/pgp-encrypted"; boundary="xyz"\r\n' +
		'\r\n--xyz\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n' +
		`--xyz\r\nContent-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\r\n${b64}\r\n--xyz--\r\n`,
	'pgp-no-protocol':
		'Content-Type: multipart/encrypted; boundary="nb"\r\n' +
		`\r\n--nb\r\nContent-Type: application/octet-stream\r\n\r\n${armor}\r\n--nb--\r\n`,
	'pgp-folded-header':
		'Content-Type: multipart/encrypted;\r\n protocol="application/pgp-encrypted";\r\n boundary="fold"\r\n' +
		`\r\n--fold\r\nContent-Type: application/octet-stream\r\n\r\n${armor}\r\n--fold--\r\n`,
	'pgp-missing-boundary':
		'Content-Type: multipart/encrypted; protocol="application/pgp-encrypted"\r\n\r\nnothing\r\n',
	'pgp-no-armor':
		'Content-Type: multipart/encrypted; boundary="na"\r\n' +
		'\r\n--na\r\nContent-Type: application/octet-stream\r\n\r\nnot armored at all\r\n--na--\r\n',
	'multipart-not-encrypted':
		'Content-Type: multipart/alternative; boundary="m"\r\n' +
		'\r\n--m\r\nContent-Type: text/plain\r\n\r\nhello\r\n--m--\r\n'
};

const meta = {};
for (const [name, mime] of Object.entries(cases)) {
	writeFileSync(new URL(`${name}.eml`, dir), mime, 'utf8');
	meta[name] = {
		isPgpEncrypted: isPgpEncryptedMime(mime),
		armor: extractPgpArmor(mime)
	};
}
writeFileSync(new URL('meta.json', dir), JSON.stringify(meta, null, '\t') + '\n');
console.log(`wrote ${Object.keys(cases).length} mime cases`);
