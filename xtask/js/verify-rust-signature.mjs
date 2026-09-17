import { readFileSync } from 'node:fs';

const openpgp = await import(
	new URL('../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const [dir, expectedFingerprint] = process.argv.slice(2);

const publicKey = await openpgp.readKey({ armoredKey: readFileSync(`${dir}/rust-signer.pub.asc`, 'utf8') });
const data = new Uint8Array(readFileSync(`${dir}/rust-signed.bin`));
const signature = await openpgp.readSignature({
	binarySignature: new Uint8Array(readFileSync(`${dir}/rust-signature.sig`))
});
const result = await openpgp.verify({
	message: await openpgp.createMessage({ binary: data }),
	signature,
	verificationKeys: publicKey
});
await result.signatures[0].verified;
if (publicKey.getFingerprint().toLowerCase() !== expectedFingerprint.toLowerCase()) {
	throw new Error('signer fingerprint mismatch');
}
const armored = signature.armor();
if (!armored.startsWith('-----BEGIN PGP SIGNATURE-----')) {
	throw new Error('signature does not armor');
}
