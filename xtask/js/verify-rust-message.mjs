import { readFileSync } from 'node:fs';

const openpgp = await import(
	new URL('../../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const [dir, passphrase, expected] = process.argv.slice(2);

const locked = await openpgp.readPrivateKey({
	armoredKey: readFileSync(`${dir}/rust-key.enc.asc`, 'utf8')
});
const key = await openpgp.decryptKey({ privateKey: locked, passphrase });

const message = await openpgp.readMessage({
	binaryMessage: new Uint8Array(readFileSync(`${dir}/rust-message.pgp`))
});
const { data } = await openpgp.decrypt({ message, decryptionKeys: key });

if (data !== expected) {
	throw new Error(`plaintext mismatch: ${JSON.stringify(data)} !== ${JSON.stringify(expected)}`);
}
