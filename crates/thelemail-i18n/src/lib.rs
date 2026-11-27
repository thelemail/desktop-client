mod locale;
pub mod messages;
pub mod plural;
pub mod rich;

pub use locale::{Locale, locale, set_locale};
pub use messages as m;
pub use rich::{Rich, Run};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_messages_follow_the_current_locale() {
        let cancel: Vec<_> = Locale::ALL
            .into_iter()
            .map(|l| {
                set_locale(l);
                m::common_cancel()
            })
            .collect();
        assert_eq!(cancel, ["Cancel", "Abbrechen", "Annuler", "Cancelar"]);

        set_locale(Locale::Pt);
        assert_eq!(m::mail_thread_count(1), "1 mensagem nesta conversa");
        assert_eq!(m::mail_thread_count(0), "0 mensagens nesta conversa");
        assert_eq!(m::cal_op_replied("accepted"), "Respondeu que aceita");

        set_locale(Locale::Fr);
        assert_eq!(m::mail_thread_count(0), "0 message dans cette conversation");
        assert_eq!(
            m::mail_thread_count(2),
            "2 messages dans cette conversation"
        );

        set_locale(Locale::De);
        assert_eq!(
            m::mail_thread_count(1),
            "1 Nachricht in dieser Unterhaltung"
        );
        assert_eq!(
            m::mail_thread_count(0),
            "0 Nachrichten in dieser Unterhaltung"
        );

        set_locale(Locale::En);
        let rollovers: Vec<_> = [1, 2, 3, 4, 11, 22]
            .into_iter()
            .map(m::cal_task_rollover)
            .collect();
        assert_eq!(
            rollovers,
            [
                "1st rollover",
                "2nd rollover",
                "3rd rollover",
                "4th rollover",
                "11th rollover",
                "22nd rollover"
            ]
        );
        assert_eq!(m::cal_op_replied("declined"), "Replied declined");
        assert_eq!(m::cal_op_replied("delegated"), "Replied delegated");
        assert_eq!(m::mail_send_err_wait_minutes(1), "about a minute");
        assert_eq!(m::mail_send_err_wait_minutes(5), "about 5 minutes");

        let rich = m::mail_alerts_dns_detail_more("a.example", 2);
        assert_eq!(rich.runs()[1].tag, Some("mono"));
        assert_eq!(rich.runs()[1].text, "a.example");
        assert!(
            rich.plain().contains("and 2 more domains"),
            "{}",
            rich.plain()
        );
    }
}
