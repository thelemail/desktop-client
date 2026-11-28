import { mkdirSync, writeFileSync } from 'node:fs';
import {
	isFreePlan,
	isFreeFamily,
	allowsCustomDomains,
	allowsSharedAddresses,
	allowsSharedDomainAlias,
	allowsMembers,
	inviteMode
} from '../../web-client/packages/core/src/settings/entitlements.ts';

const dir = new URL('../../fixtures/entitlements/', import.meta.url);
mkdirSync(dir, { recursive: true });

const codes = [null, 'free', 'free_family', 'personal', 'personal_plus', 'family', 'family_plus', 'team', 'business', 'unknown_plan'];
const types = [null, 'personal', 'family', 'business', 'unknown_type'];

const plans = codes.map((code) => ({
	code,
	isFreePlan: isFreePlan(code),
	isFreeFamily: isFreeFamily(code),
	allowsCustomDomains: allowsCustomDomains(code),
	allowsSharedAddresses: allowsSharedAddresses(code),
	allowsSharedDomainAlias: allowsSharedDomainAlias(code),
	allowsMembers: allowsMembers(code)
}));
const invites = types.flatMap((type) => codes.map((code) => ({ type, code, mode: inviteMode(type, code) })));

writeFileSync(new URL('cases.json', dir), JSON.stringify({ plans, invites }, null, '\t') + '\n');
console.log(`wrote ${plans.length} plans and ${invites.length} invite cases`);
