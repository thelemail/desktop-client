import { readFileSync } from 'node:fs';

const openpgp = await import(
	new URL('../../web-client/node_modules/openpgp/dist/node/openpgp.mjs', import.meta.url).href
);

const [dir, passphrase, expectedFingerprint] = process.argv.slice(2);

const locked = await openpgp.readPrivateKey({
	armoredKey: readFileSync(`${dir}/rust-key.enc.asc`, 'utf8')
});
const unlocked = await openpgp.decryptKey({ privateKey: locked, passphrase });

const fingerprint = unlocked.getFingerprint();
if (fingerprint !== expectedFingerprint) {
	throw new Error(`fingerprint mismatch: ${fingerprint} !== ${expectedFingerprint}`);
}
if (unlocked.keyPacket.version !== 6) {
	throw new Error(`primary key packet version ${unlocked.keyPacket.version}, expected 6`);
}
if (unlocked.keyPacket.algorithm !== 27) {
	throw new Error(`primary algorithm ${unlocked.keyPacket.algorithm}, expected 27 (ed25519)`);
}
const sub = unlocked.subkeys[0].keyPacket;
if (sub.version !== 6 || sub.algorithm !== 25) {
	throw new Error(`subkey version/algorithm ${sub.version}/${sub.algorithm}, expected 6/25 (x25519)`);
}
const features = unlocked.directSignatures[0]?.features?.[0] ?? 0;
if ((features & openpgp.enums.features.seipdv2) === 0) {
	throw new Error(`key does not advertise SEIPDv2 (features 0x${features.toString(16)})`);
}

const pub = await openpgp.readKey({ armoredKey: readFileSync(`${dir}/rust-key.pub.asc`, 'utf8') });
const message = await openpgp.createMessage({ text: 'round trip through openpgp.js' });
const encrypted = await openpgp.encrypt({ message, encryptionKeys: pub, format: 'binary' });
const { data } = await openpgp.decrypt({
	message: await openpgp.readMessage({ binaryMessage: encrypted }),
	decryptionKeys: unlocked
});
if (data !== 'round trip through openpgp.js') {
	throw new Error('decrypted plaintext did not match');
}
const seipd = (await openpgp.readMessage({ binaryMessage: encrypted })).packets.filterByTag(
	openpgp.enums.packet.symEncryptedIntegrityProtectedData
)[0];
if (seipd.version !== 2) {
	throw new Error(`openpgp.js encrypted to the Rust key with SEIPD v${seipd.version}, expected v2`);
}
