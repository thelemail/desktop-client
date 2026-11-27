use thelemail_i18n::m;

use crate::api::billing::{Plan, PlanCode, PricingModel, WorkspaceType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteMode {
    None,
    Domain,
    ExistingAccount,
}

pub fn is_free_plan(code: Option<&PlanCode>) -> bool {
    matches!(code, Some(PlanCode::Free | PlanCode::FreeFamily))
}

pub fn is_free_family(code: Option<&PlanCode>) -> bool {
    matches!(code, Some(PlanCode::FreeFamily))
}

pub fn allows_custom_domains(code: Option<&PlanCode>) -> bool {
    code.is_some() && !is_free_plan(code)
}

pub fn allows_shared_domain_alias(code: Option<&PlanCode>) -> bool {
    code.is_some_and(|c| {
        !matches!(
            c,
            PlanCode::Free | PlanCode::Personal | PlanCode::PersonalPlus
        )
    })
}

pub fn allows_shared_addresses(code: Option<&PlanCode>) -> bool {
    code.is_some() && (!is_free_plan(code) || allows_shared_domain_alias(code))
}

pub fn allows_members(code: Option<&PlanCode>) -> bool {
    code.is_some_and(|c| *c != PlanCode::Free)
}

pub fn invite_mode(kind: Option<&WorkspaceType>, code: Option<&PlanCode>) -> InviteMode {
    if !matches!(kind, Some(WorkspaceType::Family | WorkspaceType::Business)) {
        return InviteMode::None;
    }
    if !allows_members(code) {
        return InviteMode::None;
    }
    if is_free_family(code) {
        InviteMode::ExistingAccount
    } else {
        InviteMode::Domain
    }
}

pub fn plan_label(kind: Option<&WorkspaceType>, code: Option<&PlanCode>) -> String {
    match code {
        Some(PlanCode::FreeFamily) => return m::settings_plan_free_family().to_owned(),
        Some(PlanCode::Free) => return m::settings_plan_free().to_owned(),
        _ => {}
    }
    match kind {
        Some(WorkspaceType::Personal) => m::settings_plan_personal().to_owned(),
        Some(WorkspaceType::Family) => m::settings_plan_family().to_owned(),
        Some(WorkspaceType::Business) => m::settings_plan_business().to_owned(),
        _ => String::new(),
    }
}

pub fn seat_limit_for(plan: Option<&Plan>, subscribed_seats: Option<u32>) -> Option<u32> {
    let plan = plan?;
    if plan.pricing_model == PricingModel::PerMailbox {
        return subscribed_seats;
    }
    plan.max_seats
}

pub fn is_invitable(kind: Option<&WorkspaceType>) -> bool {
    matches!(kind, Some(WorkspaceType::Family | WorkspaceType::Business))
}

pub fn membership_title(kind: Option<&WorkspaceType>) -> &'static str {
    match kind {
        Some(WorkspaceType::Family) => m::settings_member_title_family(),
        Some(WorkspaceType::Business) => m::settings_member_title_business(),
        _ => m::settings_member_title_personal(),
    }
}

pub fn add_member_label(kind: Option<&WorkspaceType>) -> &'static str {
    match kind {
        Some(WorkspaceType::Family) => m::settings_member_add_family(),
        _ => m::settings_member_add(),
    }
}

pub fn seats_full_note(
    kind: Option<&WorkspaceType>,
    total: Option<u32>,
    code: Option<&PlanCode>,
) -> Option<String> {
    if kind == Some(&WorkspaceType::Family) {
        let total = total?;
        return Some(if is_free_family(code) {
            m::settings_member_seats_full_free_family(total)
        } else {
            m::settings_member_seats_full_family(total)
        });
    }
    Some(m::settings_member_seats_prorated().to_owned())
}

pub fn personal_note() -> &'static str {
    m::settings_member_personal_note()
}

pub fn free_note(
    kind: Option<&WorkspaceType>,
    code: Option<&PlanCode>,
    plan: Option<&Plan>,
    domain: &str,
) -> Option<String> {
    let plan = plan?;
    let storage = format_bytes(plan.storage_bytes_per_mailbox);
    let family = is_free_family(code)
        || (kind == Some(&WorkspaceType::Family) && code != Some(&PlanCode::Free));
    if family {
        let seats = plan.max_seats?;
        return Some(m::settings_plan_free_family_note(seats, domain, storage));
    }
    Some(m::settings_plan_free_note(domain, storage))
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= GB {
        m::lc_bytes_gb(tenths(bytes, GB))
    } else if bytes >= MB {
        m::lc_bytes_mb(tenths(bytes, MB))
    } else if bytes >= KB {
        m::lc_bytes_kb(tenths(bytes, KB))
    } else {
        m::lc_bytes_bytes(bytes)
    }
}

fn tenths(bytes: u64, unit: u64) -> String {
    let scaled = u128::from(bytes) * 10;
    let unit = u128::from(unit);
    let mut q = scaled / unit;
    if (scaled % unit) * 2 >= unit {
        q += 1;
    }
    if q % 10 == 0 {
        (q / 10).to_string()
    } else {
        format!("{}.{}", q / 10, q % 10)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use thelemail_i18n::{Locale, set_locale};

    use super::*;
    use crate::api::billing::PrioritySupport;

    fn code(v: &Value) -> Option<PlanCode> {
        v.as_str().map(|s| PlanCode::from(s.to_owned()))
    }

    fn kind(v: &Value) -> Option<WorkspaceType> {
        v.as_str().map(|s| WorkspaceType::from(s.to_owned()))
    }

    fn fixture() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/entitlements/cases.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
    }

    #[test]
    fn entitlements_match_the_web_for_every_plan() {
        let cases = fixture();
        for case in cases["plans"].as_array().unwrap() {
            let c = code(&case["code"]);
            let c = c.as_ref();
            let name = &case["code"];
            assert_eq!(is_free_plan(c), case["isFreePlan"], "{name}");
            assert_eq!(is_free_family(c), case["isFreeFamily"], "{name}");
            assert_eq!(
                allows_custom_domains(c),
                case["allowsCustomDomains"],
                "{name}"
            );
            assert_eq!(
                allows_shared_addresses(c),
                case["allowsSharedAddresses"],
                "{name}"
            );
            assert_eq!(
                allows_shared_domain_alias(c),
                case["allowsSharedDomainAlias"],
                "{name}"
            );
            assert_eq!(allows_members(c), case["allowsMembers"], "{name}");
        }
    }

    #[test]
    fn invite_modes_match_the_web() {
        let cases = fixture();
        for case in cases["invites"].as_array().unwrap() {
            let mode = invite_mode(kind(&case["type"]).as_ref(), code(&case["code"]).as_ref());
            let want = match case["mode"].as_str().unwrap() {
                "none" => InviteMode::None,
                "domain" => InviteMode::Domain,
                "existing-account" => InviteMode::ExistingAccount,
                other => panic!("unknown mode {other}"),
            };
            assert_eq!(mode, want, "{case}");
        }
    }

    fn plan(pricing: PricingModel, max_seats: Option<u32>) -> Plan {
        Plan {
            code: PlanCode::FreeFamily,
            workspace_type: WorkspaceType::Family,
            display_name: "Free family".into(),
            pricing_model: pricing,
            currency: "eur".into(),
            yearly_amount_cents: 0,
            monthly_amount_cents: 0,
            min_seats: 1,
            max_seats,
            storage_bytes_per_mailbox: 5 * 1024 * 1024 * 1024,
            custom_domains_limit: 0,
            audit_retention_days: 0,
            priority_support: PrioritySupport::None,
        }
    }

    #[test]
    fn plan_display_follows_the_web_rules() {
        set_locale(Locale::En);
        let family = Some(&WorkspaceType::Family);
        let business = Some(&WorkspaceType::Business);
        assert_eq!(
            plan_label(family, Some(&PlanCode::FreeFamily)),
            "Free family"
        );
        assert_eq!(
            plan_label(Some(&WorkspaceType::Personal), Some(&PlanCode::Free)),
            "Free"
        );
        assert_eq!(plan_label(business, Some(&PlanCode::Team)), "Business");
        assert_eq!(plan_label(None, None), "");

        let flat = plan(PricingModel::Flat, Some(6));
        let seats = plan(PricingModel::PerMailbox, Some(6));
        assert_eq!(seat_limit_for(None, Some(3)), None);
        assert_eq!(seat_limit_for(Some(&flat), Some(3)), Some(6));
        assert_eq!(seat_limit_for(Some(&seats), Some(3)), Some(3));
        assert_eq!(seat_limit_for(Some(&seats), None), None);

        assert!(is_invitable(family) && is_invitable(business));
        assert!(!is_invitable(Some(&WorkspaceType::Personal)) && !is_invitable(None));
        assert_eq!(membership_title(family), "Household");
        assert_eq!(membership_title(None), m::settings_member_title_personal());
        assert_eq!(add_member_label(family), "Add a family member");
        assert_eq!(add_member_label(business), "Add member");

        assert_eq!(seats_full_note(family, None, None), None);
        assert_eq!(
            seats_full_note(family, Some(6), Some(&PlanCode::FreeFamily)).unwrap(),
            "All 6 seats are in use. Remove someone before inviting anyone else."
        );
        assert_eq!(
            seats_full_note(family, Some(6), Some(&PlanCode::Family)).unwrap(),
            "All 6 included seats are in use. A larger team needs a Business plan."
        );
        assert_eq!(
            seats_full_note(business, None, None).unwrap(),
            m::settings_member_seats_prorated()
        );

        assert_eq!(
            free_note(family, Some(&PlanCode::Free), None, "thelemail.test"),
            None
        );
        assert_eq!(
            free_note(
                family,
                Some(&PlanCode::FreeFamily),
                Some(&flat),
                "thelemail.test"
            )
            .unwrap(),
            "Up to 6 accounts, each with an address on thelemail.test and 5 GB of storage. Free, with no custom domains."
        );
        assert_eq!(
            free_note(
                family,
                Some(&PlanCode::FreeFamily),
                Some(&plan(PricingModel::Flat, None)),
                "x"
            ),
            None
        );
        assert_eq!(
            free_note(
                Some(&WorkspaceType::Personal),
                Some(&PlanCode::Free),
                Some(&flat),
                "thelemail.test"
            )
            .unwrap(),
            "The Free plan covers one person, with an address on thelemail.test and 5 GB of storage."
        );
        assert_eq!(format_bytes(512), m::lc_bytes_bytes(512));
        assert_eq!(format_bytes(1536), m::lc_bytes_kb("1.5"));
    }

    #[test]
    fn tenths_round_half_up_like_to_fixed() {
        assert_eq!(tenths(1024 * 1024 * 1024, 1024 * 1024 * 1024), "1");
        assert_eq!(tenths(1280 * 1024 * 1024, 1024 * 1024 * 1024), "1.3");
        assert_eq!(tenths(1536, 1024), "1.5");
        assert_eq!(
            tenths(15 * 1024 * 1024 * 1024 / 10, 1024 * 1024 * 1024),
            "1.5"
        );
        assert_eq!(tenths(1023 * 1024, 1024 * 1024), "1");
    }
}
