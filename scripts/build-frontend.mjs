import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, readFileSync, rmSync, utimesSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const submodule = resolve(root, 'web-client');
const dist = resolve(root, 'frontend-dist');
const pinned = readFileSync(resolve(root, 'web-client.version'), 'utf8').trim();
const strict = process.env.THELEMAIL_RELEASE === '1';

function fail(message) {
	console.error(`build-frontend: ${message}`);
	process.exit(1);
}

function warn(message) {
	if (strict) fail(message);
	console.warn(`build-frontend: ${message}`);
}

function git(cwd, args) {
	return execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
}

const override = process.env.THELEMAIL_WEB_CLIENT_DIR;
const source = override ? resolve(override) : submodule;

if (!existsSync(resolve(source, 'package.json'))) {
	fail(
		override
			? `THELEMAIL_WEB_CLIENT_DIR points at ${source}, which is not a web-client checkout`
			: 'the web-client submodule is missing — run: git submodule update --init'
	);
}

if (override) {
	warn(`building from a local checkout at ${source} instead of the pinned ${pinned}`);
} else {
	let describes = '';
	try {
		describes = git(source, ['describe', '--tags', '--exact-match']);
	} catch {
		describes = git(source, ['rev-parse', '--short', 'HEAD']);
	}
	if (describes !== pinned) {
		warn(`the submodule is at ${describes} but web-client.version pins ${pinned}`);
	}
	if (git(source, ['status', '--porcelain'])) {
		warn('the web-client submodule has uncommitted changes');
	}
	console.log(`build-frontend: using pinned web-client ${describes}`);
}

function buildConfig() {
	const config = {};
	const configFile = resolve(source, '.github/build-config.env');
	if (existsSync(configFile)) {
		for (const line of readFileSync(configFile, 'utf8').split('\n')) {
			const at = line.indexOf('=');
			if (at > 0 && !line.startsWith('#')) config[line.slice(0, at)] = line.slice(at + 1);
		}
	}
	const roots = {
		PUBLIC_DIRECTORY_SIGNING_PUBLIC_KEY_ARMORED: 'trust-roots/directory-signing-key.asc',
		PUBLIC_TLOG_POLICY: 'trust-roots/tlog-policy.json',
		PUBLIC_OFFICIAL_SENDER_POLICY: 'trust-roots/official-sender.json'
	};
	for (const [name, file] of Object.entries(roots)) {
		const path = resolve(source, file);
		if (existsSync(path)) config[name] = readFileSync(path, 'utf8').trim();
	}
	for (const name of Object.keys(config)) {
		if (process.env[name]) delete config[name];
	}
	return config;
}

const config = override ? {} : buildConfig();
if (!override) {
	const names = Object.keys(config);
	if (names.length) console.log(`build-frontend: pinned trust roots and origins from ${pinned}`);
	else warn('the pinned checkout carries no build config — the build will use ambient environment');
}

for (const dir of [root, source]) {
	if (!existsSync(resolve(dir, 'node_modules'))) {
		execFileSync('pnpm', ['install', '--frozen-lockfile', '--ignore-scripts'], {
			cwd: dir,
			stdio: 'inherit'
		});
	}
}

execFileSync('pnpm', ['build'], {
	cwd: source,
	stdio: 'inherit',
	env: {
		...process.env,
		...config,
		PUBLIC_THELEMAIL_TARGET: 'desktop',
		THELEMAIL_PLATFORM_DIR: resolve(root, 'overlay')
	}
});

rmSync(dist, { recursive: true, force: true });
cpSync(resolve(source, 'build'), dist, { recursive: true });

const now = new Date();
for (const file of ['src-tauri/build.rs', 'src-tauri/src/main.rs']) {
	utimesSync(resolve(root, file), now, now);
}
