import { readFileSync, writeFileSync } from 'node:fs';

const opaque = await import(
	new URL('../../web-client/node_modules/@serenity-kit/opaque/cjs/index.js', import.meta.url).href
);
await opaque.default.ready;
const { client, server } = opaque.default;

const fixture = new URL('../../fixtures/opaque/setup.json', import.meta.url);
const [op] = process.argv.slice(2);

const CLIENT_IDENTITY = 'thelemail/auth/opaque/v1:11111111-2222-3333-4444-555555555555';
const SERVER_IDENTITY = 'thelemail.com';
const KEY_STRETCHING = 'memory-constrained';
const PASSWORD = 'correct horse battery staple';

if (op === 'setup') {
	const serverSetup = server.createSetup();
	const { clientRegistrationState, registrationRequest } = client.startRegistration({
		password: PASSWORD
	});
	const { registrationResponse } = server.createRegistrationResponse({
		serverSetup,
		userIdentifier: CLIENT_IDENTITY,
		registrationRequest
	});
	const { registrationRecord, exportKey } = client.finishRegistration({
		clientRegistrationState,
		registrationResponse,
		password: PASSWORD,
		identifiers: { client: CLIENT_IDENTITY, server: SERVER_IDENTITY },
		keyStretching: KEY_STRETCHING
	});
	writeFileSync(
		fixture,
		JSON.stringify(
			{
				serverSetup,
				registrationRecord,
				exportKey,
				password: PASSWORD,
				clientIdentity: CLIENT_IDENTITY,
				serverIdentity: SERVER_IDENTITY,
				keyStretching: KEY_STRETCHING
			},
			null,
			'\t'
		) + '\n'
	);
	console.log('exportKey', exportKey);
} else if (op === 'login') {
	const f = JSON.parse(readFileSync(fixture, 'utf8'));
	const ke1 = process.argv[3];
	const { serverLoginState, loginResponse } = server.startLogin({
		serverSetup: f.serverSetup,
		userIdentifier: f.clientIdentity,
		registrationRecord: f.registrationRecord,
		startLoginRequest: ke1,
		identifiers: { client: f.clientIdentity, server: f.serverIdentity }
	});
	process.stdout.write(JSON.stringify({ ke2: loginResponse, serverLoginState }));
} else if (op === 'verify') {
	const f = JSON.parse(readFileSync(fixture, 'utf8'));
	const { serverLoginState, ke3 } = JSON.parse(process.argv[3]);
	const { sessionKey } = server.finishLogin({ serverLoginState, finishLoginRequest: ke3 });
	process.stdout.write(JSON.stringify({ sessionKey }));
}
