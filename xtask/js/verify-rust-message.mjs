import { readFileSync } from 'node:fs';

const openpgp = await import(
	new URL('../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const [dir, passphrase, expected, expectSigner] = process.argv.slice(2);

const locked = await openpgp.readPrivateKey({
	armoredKey: readFileSync(`${dir}/rust-key.enc.asc`, 'utf8')
});
const key = await openpgp.decryptKey({ privateKey: locked, passphrase });

const message = await openpgp.readMessage({
	binaryMessage: new Uint8Array(readFileSync(`${dir}/rust-message.pgp`))
});
const { data, signatures } = await openpgp.decrypt({
	message,
	decryptionKeys: key,
	verificationKeys: expectSigner && expectSigner !== 'unsigned' ? key.toPublic() : undefined,
	expectSigned: false
});

if (data !== expected) {
	throw new Error(`plaintext mismatch: ${JSON.stringify(data)} !== ${JSON.stringify(expected)}`);
}

if (expectSigner === 'unsigned') {
	if (signatures.length !== 0) {
		throw new Error(`expected no signature, found ${signatures.length}`);
	}
} else if (expectSigner) {
	if (signatures.length !== 1) {
		throw new Error(`expected exactly one signature, found ${signatures.length}`);
	}
	await signatures[0].verified;
	const keyId = signatures[0].keyID.toHex().toLowerCase();
	if (!expectSigner.toLowerCase().endsWith(keyId)) {
		throw new Error(`signature is from ${keyId}, expected a key ending ${expectSigner}`);
	}
}
