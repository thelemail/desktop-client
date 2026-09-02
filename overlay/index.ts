import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

function report(kind: string, message: string, detail?: string) {
	void invoke('ui_diagnostic', { report: { kind, message, detail: detail ?? null } }).catch(
		() => {}
	);
}

if (typeof window !== 'undefined') {
	window.addEventListener('error', (ev) => {
		report('error', ev.message ?? 'unknown', ev.error?.stack);
	});
	window.addEventListener('unhandledrejection', (ev) => {
		const reason = ev.reason;
		report(
			'rejection',
			reason instanceof Error ? reason.message : String(reason),
			reason instanceof Error ? reason.stack : undefined
		);
	});
	const runProbe = () => {
		try {
			const frame = document.createElement('iframe');
			frame.setAttribute('sandbox', 'allow-same-origin');
			frame.setAttribute(
				'srcdoc',
				'<!doctype html><html><head><meta http-equiv="Content-Security-Policy" ' +
					'content="default-src \'none\'; style-src \'unsafe-inline\'; img-src data: cid:; ' +
					'script-src \'none\'"><style>body{color:#222}</style></head>' +
					'<body><p>PROBE_OK</p></body></html>'
			);
			frame.style.cssText = 'position:absolute;width:1px;height:1px;opacity:0;pointer-events:none';
			frame.addEventListener('load', () => {
				let outcome: string;
				try {
					const doc = frame.contentDocument;
					outcome = doc ? `text=${JSON.stringify(doc.body.textContent)}` : 'NULL_CONTENTDOC';
				} catch (err) {
					outcome = 'THREW:' + (err instanceof Error ? err.message : String(err));
				}
				report('probe', `srcdoc iframe ${outcome}`);
				frame.remove();
			});
			document.body.appendChild(frame);
		} catch (err) {
			report('probe', 'probe failed', err instanceof Error ? err.stack : undefined);
		}
	};

	if (document.readyState === 'loading') {
		document.addEventListener('DOMContentLoaded', runProbe, { once: true });
	} else {
		setTimeout(runProbe, 0);
	}

	window.addEventListener('securitypolicyviolation', (ev) => {
		report(
			'csp',
			`${ev.violatedDirective} blocked ${ev.blockedURI}`,
			`from ${ev.sourceFile ?? 'unknown'}:${ev.lineNumber ?? 0}`
		);
	});
}
import { env } from '$env/dynamic/public';

interface NativeResponse {
	status: number;
	headers: Record<string, string>;
	body: number[] | null;
}

async function nativeRequest(
	url: string,
	init: RequestInit,
	kind: 'api' | 'submission' = 'api'
): Promise<Response> {
	const headers: Record<string, string> = {};
	const source = init.headers as Record<string, string> | undefined;
	if (source) {
		for (const [k, v] of Object.entries(source)) headers[k] = v;
	}
	headers['X-Client'] = 'desktop';
	const command = kind === 'submission' ? 'submission_request' : 'api_request';
	const res = await invoke<NativeResponse>(command, {
		req: {
			url,
			method: init.method ?? 'GET',
			headers,
			body: typeof init.body === 'string' ? init.body : null
		}
	});
	return new Response(res.body === null ? null : new Uint8Array(res.body), {
		status: res.status,
		headers: res.headers
	});
}

async function nativeBlobPut(
	url: string,
	body: Blob,
	contentType?: string,
	opts?: { signal?: AbortSignal; onProgress?: (fraction: number) => void }
): Promise<Response> {
	opts?.signal?.throwIfAborted();
	const bytes = Array.from(new Uint8Array(await body.arrayBuffer()));
	const status = await invoke<number>('blob_put', {
		args: { url, bytes, contentType: contentType ?? body.type ?? null }
	});
	opts?.onProgress?.(1);
	return new Response(null, { status });
}

async function nativeBlobFetch(url: string): Promise<Response> {
	const bytes = await invoke<number[]>('blob_get', { url });
	return new Response(new Uint8Array(bytes), { status: 200 });
}

const KEYSTORE_COMMANDS: Record<string, string> = {
	status: 'keystore_status',
	opaqueStartAuth: 'keystore_opaque_start_auth',
	opaqueFinishAuth: 'keystore_opaque_finish_auth',
	opaqueCompleteLoginUnlock: 'keystore_opaque_complete_login_unlock',
	opaqueAbandonOperation: 'keystore_opaque_abandon_operation',
	opaqueStartRegistration: 'keystore_opaque_start_registration',
	opaqueFinishRegistration: 'keystore_opaque_finish_registration',
	opaqueFinalizeRegister: 'keystore_opaque_finalize_register',
	enrollPersistent: 'keystore_enroll_persistent',
	invalidatePersistedVault: 'keystore_invalidate_persisted_vault',
	tryRestoreFromPersistent: 'keystore_try_restore_from_persistent',
	disablePersistent: 'keystore_disable_persistent',
	clear: 'keystore_clear',
	lock: 'keystore_lock',
	clearAll: 'keystore_clear_all',
	decrypt: 'keystore_decrypt',
	loadAliasKeys: 'keystore_load_alias_keys',
	unloadAliasKeys: 'keystore_unload_alias_keys',
	reformatKeyWithUids: 'keystore_reformat_key_with_uids',
	attachmentHeader: 'keystore_attachment_header',
	attachmentBytes: 'keystore_attachment_bytes',
	encrypt: 'keystore_encrypt',
	encryptToKeys: 'keystore_encrypt_to_keys',
	getPublicKey: 'keystore_get_public_key',
	opaqueRecoverySetupStart: 'keystore_opaque_recovery_setup_start',
	opaqueRecoverySetupFinish: 'keystore_opaque_recovery_setup_finish',
	opaqueCompleteRecoveryUnlock: 'keystore_opaque_complete_recovery_unlock',
	opaquePrepareCredentialReset: 'keystore_opaque_prepare_credential_reset',
	opaqueFinishCredentialReset: 'keystore_opaque_finish_credential_reset',
	opaquePasswordChangeStart: 'keystore_opaque_password_change_start',
	opaquePasswordChangeFinish: 'keystore_opaque_password_change_finish',
	opaquePasswordChangeCommit: 'keystore_opaque_password_change_commit',
	createAliasKey: 'keystore_create_alias_key',
	commitReformattedKey: 'keystore_commit_reformatted_key',
	discardRecovery: 'keystore_discard_recovery',
	abandonPasswordChange: 'keystore_abandon_password_change'
};

const SRP_COMMANDS = new Set([
	'prepareLogin',
	'verifyLoginProof',
	'completeLoginUnlock',
	'abandonLogin',
	'prepareRecoverySetup',
	'prepareRecoveryLogin',
	'verifyRecoveryProof',
	'completeRecoveryUnlock',
	'prepareCredentialReset',
	'preparePasswordChangeProof',
	'verifyPasswordChangeProof',
	'preparePasswordChangeCredentials',
	'commitPasswordChange',
	'prepareDeletionProof',
	'opaquePrepareAmkRotation',
	'opaqueFinishAmkRotation'
]);

const NOOP_COMMANDS = new Set(['migrationStartRegistration', 'migrationFinishStage']);

type BroadcastListener = (b: unknown) => void;
const keystoreListeners = new Set<BroadcastListener>();
let keystoreBound = false;

function bindKeystoreEvents() {
	if (keystoreBound) return;
	keystoreBound = true;
	void listen('keystore', (ev) => {
		for (const cb of keystoreListeners) cb(ev.payload);
	});
}

const BYTE_FIELDS = ['ciphertext', 'plaintextBinary', 'fingerprint', 'exportKey', 'sessionKey'];

function normalizeBytes<T>(value: T): T {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
	const record = value as Record<string, unknown>;
	let patched: Record<string, unknown> | null = null;
	for (const field of BYTE_FIELDS) {
		const raw = record[field];
		if (Array.isArray(raw)) {
			patched ??= { ...record };
			patched[field] = new Uint8Array(raw as number[]);
		}
	}
	return (patched ?? value) as T;
}

const keystoreChannel = {
	async call<T>(cmd: string, args?: unknown): Promise<T> {
		const command = KEYSTORE_COMMANDS[cmd];
		if (!command) {
			if (SRP_COMMANDS.has(cmd)) {
				throw new Error(
					'This account still uses our previous sign-in system. Sign in once at app.thelemail.com to upgrade it, then come back here.'
				);
			}
			if (NOOP_COMMANDS.has(cmd)) {
				return { ok: false, code: 'locked' } as T;
			}
			throw new Error(`${cmd} is not available in the desktop app yet.`);
		}
		const result = await invoke<T>(command, args === undefined ? {} : { args });
		if (cmd === 'attachmentBytes') {
			const res = result as {
				ok?: boolean;
				header?: { contentType?: string };
				payload?: number[];
			};
			if (res?.ok && Array.isArray(res.payload)) {
				return {
					...res,
					payload: new Blob([new Uint8Array(res.payload)], {
						type: res.header?.contentType ?? 'application/octet-stream'
					})
				} as T;
			}
		}
		return normalizeBytes(result);
	},
	subscribe(cb: BroadcastListener): () => void {
		bindKeystoreEvents();
		keystoreListeners.add(cb);
		return () => keystoreListeners.delete(cb);
	}
};

interface StreamFrame {
	stream: string;
	kind: 'open' | 'message' | 'error';
	data?: string;
	id?: string;
}

interface EventSourceLike {
	close(): void;
	onopen: ((ev: Event) => void) | null;
	onerror: ((ev: Event) => void) | null;
	onmessage: ((ev: MessageEvent) => void) | null;
}

function openEventSource(url: string): EventSourceLike {
	const shim: EventSourceLike = {
		close: () => {},
		onopen: null,
		onerror: null,
		onmessage: null
	};
	let streamId: string | null = null;
	let unlisten: (() => void) | null = null;
	let closed = false;

	void (async () => {
		unlisten = await listen<StreamFrame>('realtime', (ev) => {
			const frame = ev.payload;
			if (!streamId || frame.stream !== streamId) return;
			if (frame.kind === 'open') shim.onopen?.(new Event('open'));
			else if (frame.kind === 'message')
				shim.onmessage?.(
					new MessageEvent('message', { data: frame.data ?? '', lastEventId: frame.id ?? '' })
				);
			else shim.onerror?.(new Event('error'));
		});
		if (closed) {
			unlisten();
			return;
		}
		streamId = await invoke<string>('realtime_open', { args: { url } });
	})();

	shim.close = () => {
		closed = true;
		unlisten?.();
		if (streamId) void invoke('realtime_close', { args: { streamId } });
		streamId = null;
	};
	return shim;
}

function subscribe<T>(event: string, cb: (payload: T) => void): () => void {
	let stop = false;
	let unlisten: (() => void) | null = null;
	void listen<T>(event, (ev) => cb(ev.payload)).then((fn) => {
		if (stop) fn();
		else unlisten = fn;
	});
	return () => {
		stop = true;
		unlisten?.();
	};
}

const mirror = {
	open: (accountId: string) => invoke<void>('mirror_open', { args: { accountId } }),
	close: (accountId: string) => invoke<void>('mirror_close', { args: { accountId } }),
	startSync: (accountId: string, accessToken: string) =>
		invoke<void>('mirror_start_sync', { args: { accountId, accessToken } }),
	setToken: (accountId: string, accessToken: string) =>
		invoke<void>('mirror_set_token', { args: { accountId, accessToken } }),
	stopWatch: (accountId: string) => invoke<void>('mirror_stop_watch', { args: { accountId } }),
	search: (accountId: string, query: string, limit?: number) =>
		invoke<unknown[]>('mirror_search', { args: { accountId, query, limit } }),
	list: (accountId: string, mailbox: string, direction?: string, limit?: number) =>
		invoke<unknown[]>('mirror_list', { args: { accountId, mailbox, direction, limit } }),
	scope: (accountId: string) => invoke<string | null>('mirror_scope', { args: { accountId } }),
	setScope: (accountId: string, dateFloor: string | null) =>
		invoke<void>('mirror_set_scope', { args: { accountId, dateFloor } }),
	message: (accountId: string, messageId: string) =>
		invoke<unknown>('mirror_message', { args: { accountId, messageId } }),
	thread: (accountId: string, messageId: string) =>
		invoke<unknown[]>('mirror_thread', { args: { accountId, messageId } }),
	onChanged: (cb: (accountId: string) => void) =>
		subscribe<{ accountId: string }>('mirror://changed', (ev) => cb(ev.accountId)),
	onTokenExpired: (cb: (accountId: string) => void) =>
		subscribe<{ accountId: string }>('mirror://token-expired', (ev) => cb(ev.accountId))
};

interface NotificationStatus {
	supported: boolean;
	bundled: boolean;
	translocated: boolean;
	bundlePath: string | null;
	authorization: string;
	alerts: boolean;
	sound: boolean;
	lastError: string | null;
}

interface NotificationTarget {
	accountId: string;
	messageId: string;
}

const notifications = {
	status: () => invoke<NotificationStatus>('notify_status'),
	takeOpened: () => invoke<NotificationTarget | null>('notify_take_opened'),
	onStatus: (cb: (status: NotificationStatus) => void) =>
		subscribe<NotificationStatus>('notify://status', cb),
	onOpened: (cb: (target: NotificationTarget) => void) =>
		subscribe<NotificationTarget>('notification://opened', cb)
};

const session = {
	persist: (accountId: string) =>
		invoke<boolean>('session_persist', { args: { accountId } }),
	restore: (accountId: string) =>
		invoke<boolean>('session_restore', { args: { accountId } }),
	forget: (accountId: string) => invoke<void>('session_forget', { args: { accountId } })
};

export const platform = {
	reportError: (kind: string, err: unknown) =>
		report(
			kind,
			err instanceof Error ? `${err.name}: ${err.message}` : String(err),
			err instanceof Error ? err.stack : undefined
		),
	interceptFrameLinks: true,
	writeFrameDoc: true,
	session,
	billing: 'handoff' as const,
	mirror,
	keystoreChannel,
	transport: nativeRequest,
	openEventSource,
	notifications,
	blobFetch: nativeBlobFetch,
	blobPut: nativeBlobPut,
	returnOrigin: () => env.PUBLIC_APP_URL || 'https://app.thelemail.com',
	openExternal: (url: string) => {
		void invoke('open_external', { url });
	},
	saveBlob: async (blob: Blob, filename: string) => {
		const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
		await invoke<boolean>('save_bytes', { args: { filename, bytes } });
	}
};
