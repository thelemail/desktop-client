use super::hint::{Entity, HintKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effects {
    pub poke_mirror: bool,
    pub profile: bool,
    pub settings: bool,
    pub billing: bool,
}

pub fn effects(kind: &HintKind) -> Effects {
    let source = matches!(
        kind.entity,
        Entity::Message
            | Entity::Thread
            | Entity::Mailbox
            | Entity::Draft
            | Entity::ScheduledSend
            | Entity::SharedAlias
            | Entity::MailCollection
    );
    let mut out = Effects {
        poke_mirror: source,
        ..Effects::default()
    };
    match kind.entity {
        Entity::Address | Entity::Lifecycle | Entity::Account | Entity::AccountKeys => {
            out.profile = true;
        }
        Entity::Subscription => {
            out.profile = true;
            out.billing = true;
        }
        Entity::Settings => out.settings = true,
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(kind: &str) -> Effects {
        effects(&HintKind::parse(kind))
    }

    #[test]
    fn mail_changes_poke_the_mirror_and_nothing_else() {
        for kind in [
            "message.created",
            "message.deleted",
            "thread.updated",
            "mailbox.invalidated",
            "draft.updated",
            "scheduled_send.invalidated",
            "shared_alias.created",
            "mail_collection.deleted",
        ] {
            assert_eq!(
                of(kind),
                Effects {
                    poke_mirror: true,
                    ..Effects::default()
                },
                "{kind}"
            );
        }
    }

    #[test]
    fn account_changes_reload_the_profile() {
        for kind in [
            "address.created",
            "address.deleted",
            "lifecycle.updated",
            "account.updated",
            "account_keys.updated",
        ] {
            assert_eq!(
                of(kind),
                Effects {
                    profile: true,
                    ..Effects::default()
                },
                "{kind}"
            );
        }
    }

    #[test]
    fn a_subscription_change_reloads_profile_and_billing() {
        assert_eq!(
            of("subscription.updated"),
            Effects {
                profile: true,
                billing: true,
                ..Effects::default()
            }
        );
    }

    #[test]
    fn settings_changes_reload_the_settings() {
        assert_eq!(
            of("settings.updated"),
            Effects {
                settings: true,
                ..Effects::default()
            }
        );
    }

    #[test]
    fn hints_for_screens_only_change_nothing_in_core() {
        for kind in [
            "signature.updated",
            "contact.updated",
            "blocked_sender.created",
            "calendar.invalidated",
            "calendar_item.created",
            "calendar_invitation.updated",
            "reply_presence.updated",
            "widget.spun",
        ] {
            assert_eq!(of(kind), Effects::default(), "{kind}");
        }
    }
}
