use serde::Deserialize;

use super::wire::wire_enum;
use super::{Api, ApiError, Call};

wire_enum!(PlanCode {
    Free => "free",
    FreeFamily => "free_family",
    Personal => "personal",
    PersonalPlus => "personal_plus",
    Family => "family",
    FamilyPlus => "family_plus",
    Team => "team",
    Business => "business",
});

wire_enum!(WorkspaceType {
    Personal => "personal",
    Family => "family",
    Business => "business",
});

wire_enum!(PricingModel {
    Flat => "flat",
    PerMailbox => "per_mailbox",
});

wire_enum!(BillingInterval {
    Month => "month",
    Year => "year",
});

wire_enum!(SubscriptionStatus {
    None => "none",
    Active => "active",
    PastDue => "past_due",
    Canceled => "canceled",
});

wire_enum!(BillingProvider {
    Stripe => "stripe",
    GooglePlay => "google_play",
    Apple => "apple",
});

wire_enum!(PrioritySupport {
    None => "none",
    Priority => "priority",
    SameBusinessDay => "same_business_day",
});

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub code: PlanCode,
    pub workspace_type: WorkspaceType,
    pub display_name: String,
    pub pricing_model: PricingModel,
    pub currency: String,
    pub yearly_amount_cents: i64,
    pub monthly_amount_cents: i64,
    pub min_seats: u32,
    #[serde(default)]
    pub max_seats: Option<u32>,
    pub storage_bytes_per_mailbox: u64,
    pub custom_domains_limit: i64,
    pub audit_retention_days: i64,
    pub priority_support: PrioritySupport,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlanCatalog {
    pub plans: Vec<Plan>,
}

impl PlanCatalog {
    pub fn plan(&self, code: &PlanCode) -> Option<&Plan> {
        self.plans.iter().find(|p| &p.code == code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub status: SubscriptionStatus,
    pub entitled: bool,
    #[serde(default)]
    pub plan_code: Option<PlanCode>,
    #[serde(default)]
    pub interval: Option<BillingInterval>,
    #[serde(default)]
    pub seats: Option<u32>,
    #[serde(default)]
    pub current_period_end: Option<String>,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    #[serde(default)]
    pub storage_bytes_used: Option<u64>,
    #[serde(default)]
    pub storage_bytes_limit: Option<u64>,
    #[serde(default)]
    pub provider: Option<BillingProvider>,
    #[serde(default)]
    pub downgrade_eligible: Option<bool>,
    #[serde(default)]
    pub pending_plan_code: Option<PlanCode>,
    #[serde(default)]
    pub pending_plan_effective_at: Option<String>,
}

pub async fn plans(api: Api<'_>) -> Result<PlanCatalog, ApiError> {
    api.public(Call::get("/v1/billing/plans")).await
}

pub async fn subscription(api: Api<'_>, account_id: &str) -> Result<Subscription, ApiError> {
    api.json(account_id, Call::get("/v1/billing/subscription"))
        .await
}
