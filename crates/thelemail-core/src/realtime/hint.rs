use serde::Deserialize;
use serde_json::Value;

use crate::api::wire::wire_enum;

wire_enum!(Entity {
    Message => "message",
    Thread => "thread",
    Mailbox => "mailbox",
    Draft => "draft",
    ScheduledSend => "scheduled_send",
    Contact => "contact",
    Address => "address",
    Signature => "signature",
    Settings => "settings",
    Account => "account",
    AccountKeys => "account_keys",
    Lifecycle => "lifecycle",
    Subscription => "subscription",
    BlockedSender => "blocked_sender",
    Calendar => "calendar",
    CalendarItem => "calendar_item",
    CalendarInvitation => "calendar_invitation",
    SharedAlias => "shared_alias",
    ReplyPresence => "reply_presence",
    MailCollection => "mail_collection",
});

wire_enum!(Action {
    Created => "created",
    Updated => "updated",
    Deleted => "deleted",
    Invalidated => "invalidated",
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HintKind {
    pub entity: Entity,
    pub action: Action,
}

impl HintKind {
    pub fn parse(kind: &str) -> Self {
        let (entity, action) = kind.split_once('.').unwrap_or((kind, ""));
        Self {
            entity: Entity::from(entity.to_owned()),
            action: Action::from(action.to_owned()),
        }
    }

    pub fn is(&self, entity: Entity, action: Action) -> bool {
        self.entity == entity && self.action == action
    }
}

impl std::fmt::Display for HintKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.entity.as_str(), self.action.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub account_id: String,
    pub kind: HintKind,
    pub id: Option<String>,
    pub thread_id: Option<String>,
    pub rev: Option<i64>,
}

#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    thread_id: Option<Value>,
    #[serde(default)]
    rev: Option<Value>,
}

fn text(v: Option<Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s),
        _ => None,
    }
}

impl Hint {
    pub fn parse(account_id: &str, data: &str) -> Option<Self> {
        let wire: Wire = serde_json::from_str(data).ok()?;
        let kind = wire.kind.filter(|k| !k.is_empty())?;
        Some(Self {
            account_id: account_id.to_owned(),
            kind: HintKind::parse(&kind),
            id: text(wire.id),
            thread_id: text(wire.thread_id),
            rev: wire.rev.and_then(|r| r.as_i64()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [&str; 40] = [
        "message.created",
        "message.updated",
        "message.deleted",
        "thread.updated",
        "mailbox.invalidated",
        "draft.created",
        "draft.updated",
        "draft.deleted",
        "scheduled_send.created",
        "scheduled_send.invalidated",
        "scheduled_send.deleted",
        "contact.updated",
        "address.created",
        "address.updated",
        "address.deleted",
        "signature.updated",
        "signature.deleted",
        "settings.updated",
        "account.updated",
        "account_keys.updated",
        "lifecycle.updated",
        "subscription.updated",
        "blocked_sender.created",
        "blocked_sender.deleted",
        "calendar.created",
        "calendar.updated",
        "calendar.deleted",
        "calendar.invalidated",
        "calendar_item.created",
        "calendar_item.updated",
        "calendar_item.deleted",
        "calendar_invitation.created",
        "calendar_invitation.updated",
        "shared_alias.created",
        "shared_alias.updated",
        "shared_alias.deleted",
        "reply_presence.updated",
        "mail_collection.created",
        "mail_collection.updated",
        "mail_collection.deleted",
    ];

    #[test]
    fn every_kind_the_server_sends_is_known() {
        for kind in KINDS {
            let parsed = HintKind::parse(kind);
            assert!(!matches!(parsed.entity, Entity::Other(_)), "{kind}");
            assert!(!matches!(parsed.action, Action::Other(_)), "{kind}");
            assert_eq!(parsed.to_string(), kind);
        }
    }

    #[test]
    fn a_hint_carries_its_ids_and_revision() {
        let hint = Hint::parse(
            "acct",
            r#"{"kind":"message.updated","id":"m1","thread_id":"t1","rev":42}"#,
        )
        .unwrap();
        assert_eq!(hint.account_id, "acct");
        assert!(hint.kind.is(Entity::Message, Action::Updated));
        assert_eq!(hint.id.as_deref(), Some("m1"));
        assert_eq!(hint.thread_id.as_deref(), Some("t1"));
        assert_eq!(hint.rev, Some(42));
    }

    #[test]
    fn a_frame_without_a_kind_is_dropped() {
        for data in [
            "{}",
            r#"{"kind":""}"#,
            r#"{"id":"x"}"#,
            "not json",
            "[]",
            r#"{"kind":7}"#,
        ] {
            assert_eq!(Hint::parse("acct", data), None, "{data}");
        }
    }

    #[test]
    fn an_unknown_kind_survives_for_later_clients() {
        let hint = Hint::parse("acct", r#"{"kind":"widget.spun","id":3}"#).unwrap();
        assert_eq!(hint.kind.entity, Entity::Other("widget".into()));
        assert_eq!(hint.kind.action, Action::Other("spun".into()));
        assert_eq!(hint.id, None);
        let bare = Hint::parse("acct", r#"{"kind":"ping"}"#).unwrap();
        assert_eq!(bare.kind.entity, Entity::Other("ping".into()));
    }
}
