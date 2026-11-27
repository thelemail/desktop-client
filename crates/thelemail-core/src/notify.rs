use std::sync::Mutex;

use std::sync::Arc;

use serde::Serialize;

use crate::{Core, CoreEvent};

#[derive(Clone, Debug)]
pub struct NewMail {
    pub account_id: String,
    pub message_id: String,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
}

impl NewMail {
    fn title(&self, locale: &str) -> String {
        if self.sender.is_empty() {
            crate::locale::text(locale, crate::locale::Text::NewMessage).to_owned()
        } else {
            self.sender.clone()
        }
    }

    fn subtitle(&self, locale: &str) -> String {
        if self.subject.is_empty() {
            crate::locale::text(locale, crate::locale::Text::NoSubject).to_owned()
        } else {
            self.subject.clone()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyStatus {
    pub supported: bool,
    pub bundled: bool,
    pub translocated: bool,
    pub bundle_path: Option<String>,
    pub authorization: String,
    pub alerts: bool,
    pub sound: bool,
    pub last_error: Option<String>,
}

impl NotifyStatus {
    fn unbundled() -> Self {
        Self {
            supported: true,
            bundled: false,
            translocated: false,
            bundle_path: None,
            authorization: "unbundled".to_owned(),
            alerts: false,
            sound: false,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    pub account_id: String,
    pub message_id: String,
}

static LAST_OPENED: Mutex<Option<NotificationTarget>> = Mutex::new(None);

fn remember_opened(target: NotificationTarget) {
    if let Ok(mut slot) = LAST_OPENED.lock() {
        *slot = Some(target);
    }
}

#[cfg(any(windows, test))]
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && !matches!(c, '\t' | '\n') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(any(windows, test))]
fn toast_xml(title: &str, subtitle: &str, body: &str) -> String {
    let mut lines = format!(
        "<text>{}</text><text>{}</text>",
        escape(title),
        escape(subtitle)
    );
    if !body.is_empty() {
        lines.push_str(&format!("<text>{}</text>", escape(body)));
    }
    format!(
        "<toast><visual><binding template=\"ToastGeneric\">{lines}</binding></visual>\
         <audio src=\"ms-winsoundevent:Notification.Mail\"/></toast>"
    )
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use super::{Arc, Core, CoreEvent, NewMail, NotificationTarget, NotifyStatus, remember_opened};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSetting, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    static DELEGATE: OnceLock<Retained<TapDelegate>> = OnceLock::new();
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static WARNED_UNBUNDLED: AtomicBool = AtomicBool::new(false);
    static WARNED_DENIED: AtomicBool = AtomicBool::new(false);
    static STATUS: Mutex<Option<NotifyStatus>> = Mutex::new(None);

    pub struct DelegateState {
        core: Arc<Core>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ThelemailNotificationDelegate"]
        #[ivars = DelegateState]
        struct TapDelegate;

        unsafe impl NSObjectProtocol for TapDelegate {}

        unsafe impl UNUserNotificationCenterDelegate for TapDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::Sound
                    | UNNotificationPresentationOptions::List,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                handler: &block2::DynBlock<dyn Fn()>,
            ) {
                let core = &self.ivars().core;
                let info = response.notification().request().content().userInfo();
                let read = |key: &str| -> Option<String> {
                    let value = info.valueForKey(&NSString::from_str(key))?;
                    let text: Retained<NSString> = unsafe { Retained::cast_unchecked(value) };
                    Some(text.to_string())
                };

                if let (Some(account_id), Some(message_id)) = (read("accountId"), read("messageId"))
                {
                    let target = NotificationTarget {
                        account_id,
                        message_id,
                    };
                    remember_opened(target.clone());
                    core.emit(CoreEvent::NotificationOpened(target));
                }
                handler.call(());
            }
        }
    );

    fn bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn bundle_path() -> Option<String> {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier()?;
        Some(bundle.bundlePath().to_string())
    }

    fn translocated() -> bool {
        bundle_path().is_some_and(|path| path.contains("/AppTranslocation/"))
    }

    fn center() -> Retained<UNUserNotificationCenter> {
        UNUserNotificationCenter::currentNotificationCenter()
    }

    fn describe_error(err: *mut NSError) -> Option<String> {
        let err = unsafe { err.as_ref()? };
        Some(format!(
            "{} ({}:{})",
            err.localizedDescription(),
            err.domain(),
            err.code()
        ))
    }

    fn authorization_name(status: UNAuthorizationStatus) -> &'static str {
        if status == UNAuthorizationStatus::NotDetermined {
            "notDetermined"
        } else if status == UNAuthorizationStatus::Denied {
            "denied"
        } else if status == UNAuthorizationStatus::Authorized {
            "authorized"
        } else if status == UNAuthorizationStatus::Provisional {
            "provisional"
        } else if status == UNAuthorizationStatus::Ephemeral {
            "ephemeral"
        } else {
            "unknown"
        }
    }

    fn allowed(status: UNAuthorizationStatus) -> bool {
        status == UNAuthorizationStatus::Authorized
            || status == UNAuthorizationStatus::Provisional
            || status == UNAuthorizationStatus::Ephemeral
    }

    fn snapshot(settings: &UNNotificationSettings, last_error: Option<String>) -> NotifyStatus {
        NotifyStatus {
            supported: true,
            bundled: true,
            translocated: translocated(),
            bundle_path: bundle_path(),
            authorization: authorization_name(settings.authorizationStatus()).to_owned(),
            alerts: settings.alertSetting() == UNNotificationSetting::Enabled,
            sound: settings.soundSetting() == UNNotificationSetting::Enabled,
            last_error,
        }
    }

    fn remembered_error() -> Option<String> {
        STATUS
            .lock()
            .ok()
            .and_then(|status| status.as_ref().and_then(|s| s.last_error.clone()))
    }

    fn publish(core: &Arc<Core>, status: NotifyStatus) {
        tracing::info!(
            "notifications: authorization={} alerts={} sound={} translocated={} error={}",
            status.authorization,
            status.alerts,
            status.sound,
            status.translocated,
            status.last_error.as_deref().unwrap_or("none")
        );
        if let Ok(mut slot) = STATUS.lock() {
            *slot = Some(status.clone());
        }
        core.emit(CoreEvent::NotifyStatus(status));
    }

    fn with_settings(f: impl Fn(&UNNotificationSettings) + 'static) {
        let block = block2::RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            f(unsafe { settings.as_ref() });
        });
        center().getNotificationSettingsWithCompletionHandler(&block);
    }

    fn refresh_status(core: &Arc<Core>, last_error: Option<String>) {
        let core = core.clone();
        let last_error = last_error.or_else(remembered_error);
        with_settings(move |settings| publish(&core, snapshot(settings, last_error.clone())));
    }

    pub fn bind(core: &Arc<Core>) {
        if !bundled() {
            publish(core, NotifyStatus::unbundled());
            return;
        }
        let _ = DELEGATE.get_or_init(|| {
            let this = TapDelegate::alloc().set_ivars(DelegateState { core: core.clone() });
            let delegate: Retained<TapDelegate> = unsafe { msg_send![super(this), init] };
            center().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            delegate
        });
        refresh_status(core, None);
        request_authorization(core, None);
    }

    pub fn request_authorization(core: &Arc<Core>, then: Option<NewMail>) {
        if !bundled() {
            return;
        }
        if then.is_none() && REQUESTED.swap(true, Ordering::SeqCst) {
            return;
        }
        REQUESTED.store(true, Ordering::SeqCst);
        let core = core.clone();
        let options = UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge;
        let handler =
            block2::RcBlock::new(move |granted: objc2::runtime::Bool, err: *mut NSError| {
                let error = describe_error(err);
                if !granted.as_bool() {
                    tracing::warn!(
                        "notifications: authorization was not granted: {}",
                        error.as_deref().unwrap_or("declined by the user")
                    );
                }
                refresh_status(&core, error);
                if granted.as_bool()
                    && let Some(mail) = &then
                {
                    deliver(&core, mail);
                }
            });
        center().requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    fn deliver(core: &Arc<Core>, mail: &NewMail) {
        let lang = core.locale().current();
        unsafe {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&mail.title(lang)));
            content.setSubtitle(&NSString::from_str(&mail.subtitle(lang)));
            if !mail.snippet.is_empty() {
                content.setBody(&NSString::from_str(&mail.snippet));
            }
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            content.setThreadIdentifier(&NSString::from_str(&mail.account_id));

            let keys = [
                &*NSString::from_str("accountId") as &objc2_foundation::NSString,
                &*NSString::from_str("messageId"),
            ];
            let values = [
                Retained::into_super(NSString::from_str(&mail.account_id)),
                Retained::into_super(NSString::from_str(&mail.message_id)),
            ];
            let info = NSDictionary::from_retained_objects(&keys, &values);
            let info: Retained<NSDictionary> = Retained::cast_unchecked(info);
            content.setUserInfo(&info);

            let identifier = NSString::from_str(&mail.message_id);
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &identifier,
                &content,
                None,
            );
            let core = core.clone();
            let message_id = mail.message_id.clone();
            let handler =
                block2::RcBlock::new(move |err: *mut NSError| match describe_error(err) {
                    Some(error) => {
                        tracing::warn!("notifications: the request was rejected: {error}");
                        refresh_status(&core, Some(error));
                    }
                    None => tracing::info!("notifications: posted {message_id}"),
                });
            center().addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        }
    }

    pub fn post(core: &Arc<Core>, mail: &NewMail) {
        if !bundled() {
            if !WARNED_UNBUNDLED.swap(true, Ordering::SeqCst) {
                tracing::info!(
                    "notifications: not running from an app bundle, notifications are disabled"
                );
            }
            return;
        }
        let core = core.clone();
        let mail = mail.clone();
        with_settings(move |settings| {
            let status = settings.authorizationStatus();
            tracing::info!(
                "notifications: posting {} with authorization={}",
                mail.message_id,
                authorization_name(status)
            );
            if allowed(status) {
                deliver(&core, &mail);
            } else if status == UNAuthorizationStatus::NotDetermined {
                request_authorization(&core, Some(mail.clone()));
            } else if !WARNED_DENIED.swap(true, Ordering::SeqCst) {
                refresh_status(&core, None);
            }
        });
    }

    pub async fn status() -> NotifyStatus {
        if !bundled() {
            return NotifyStatus::unbundled();
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let tx = Mutex::new(Some(tx));
        let last_error = remembered_error();
        with_settings(move |settings| {
            if let Some(tx) = tx.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = tx.send(snapshot(settings, last_error.clone()));
            }
        });
        match tokio::time::timeout(Duration::from_secs(3), rx).await {
            Ok(Ok(status)) => {
                if let Ok(mut slot) = STATUS.lock() {
                    *slot = Some(status.clone());
                }
                status
            }
            _ => STATUS
                .lock()
                .ok()
                .and_then(|slot| slot.clone())
                .unwrap_or_else(|| NotifyStatus {
                    bundled: true,
                    authorization: "unknown".to_owned(),
                    ..NotifyStatus::unbundled()
                }),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        NotificationSetting, ToastNotification, ToastNotificationManager, ToastNotifier,
    };
    use windows::core::HSTRING;

    use super::{Arc, Core, CoreEvent, NewMail, NotificationTarget, NotifyStatus, remember_opened};

    const APP_ID: &str = "com.thelemail.desktop";
    const KEPT: usize = 50;

    static SHOWN: Mutex<VecDeque<ToastNotification>> = Mutex::new(VecDeque::new());
    static WARNED_UNBUNDLED: AtomicBool = AtomicBool::new(false);
    static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

    fn installed() -> Option<String> {
        crate::updates::installed().map(|dir| dir.display().to_string())
    }

    fn notifier() -> windows::core::Result<ToastNotifier> {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
    }

    fn remember_error(error: Option<String>) -> Option<String> {
        let mut slot = LAST_ERROR.lock().ok()?;
        if error.is_some() {
            *slot = error;
        }
        slot.clone()
    }

    fn snapshot(error: Option<String>) -> NotifyStatus {
        let Some(path) = installed() else {
            return NotifyStatus::unbundled();
        };
        let mut status = NotifyStatus {
            supported: true,
            bundled: true,
            translocated: false,
            bundle_path: Some(path),
            authorization: "unknown".to_owned(),
            alerts: false,
            sound: false,
            last_error: None,
        };
        match notifier().and_then(|n| n.Setting()) {
            Ok(setting) if setting == NotificationSetting::Enabled => {
                status.authorization = "authorized".to_owned();
                status.alerts = true;
                status.sound = true;
            }
            Ok(_) => status.authorization = "denied".to_owned(),
            Err(err) => status.last_error = Some(err.message()),
        }
        if status.last_error.is_none() {
            status.last_error = remember_error(error);
        }
        status
    }

    fn publish(core: &Arc<Core>, status: NotifyStatus) {
        tracing::info!(
            "notifications: authorization={} alerts={} error={}",
            status.authorization,
            status.alerts,
            status.last_error.as_deref().unwrap_or("none")
        );
        core.emit(CoreEvent::NotifyStatus(status));
    }

    pub fn claim_app_id() {
        let id = HSTRING::from(APP_ID);
        unsafe {
            let _ =
                windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
        }
    }

    pub fn bind(core: &Arc<Core>) {
        publish(core, snapshot(None));
    }

    fn deliver(core: &Arc<Core>, mail: &NewMail) -> windows::core::Result<()> {
        let lang = core.locale().current();
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(super::toast_xml(
            &mail.title(lang),
            &mail.subtitle(lang),
            &mail.snippet,
        )))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        toast.SetTag(&HSTRING::from(
            mail.message_id.chars().take(64).collect::<String>(),
        ))?;
        toast.SetGroup(&HSTRING::from(
            mail.account_id.chars().take(64).collect::<String>(),
        ))?;

        let opened = core.clone();
        let target = NotificationTarget {
            account_id: mail.account_id.clone(),
            message_id: mail.message_id.clone(),
        };
        toast.Activated(&TypedEventHandler::new(move |_, _| {
            remember_opened(target.clone());
            opened.emit(CoreEvent::NotificationOpened(target.clone()));
            Ok(())
        }))?;

        notifier()?.Show(&toast)?;
        if let Ok(mut shown) = SHOWN.lock() {
            shown.push_back(toast);
            while shown.len() > KEPT {
                shown.pop_front();
            }
        }
        Ok(())
    }

    pub fn post(core: &Arc<Core>, mail: &NewMail) {
        if installed().is_none() {
            if !WARNED_UNBUNDLED.swap(true, Ordering::SeqCst) {
                tracing::info!(
                    "notifications: not running from an installed copy, notifications are disabled"
                );
            }
            return;
        }
        match deliver(core, mail) {
            Ok(()) => tracing::info!("notifications: posted {}", mail.message_id),
            Err(err) => {
                tracing::warn!("notifications: the toast was rejected: {}", err.message());
                publish(core, snapshot(Some(err.message())));
            }
        }
    }

    pub async fn status() -> NotifyStatus {
        tokio::task::spawn_blocking(|| snapshot(None))
            .await
            .unwrap_or_else(|_| NotifyStatus::unbundled())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::{Arc, Core, NewMail, NotifyStatus};

    pub fn bind(_core: &Arc<Core>) {}

    pub fn post(_core: &Arc<Core>, _mail: &NewMail) {}

    pub async fn status() -> NotifyStatus {
        NotifyStatus {
            supported: false,
            authorization: "unsupported".to_owned(),
            ..NotifyStatus::unbundled()
        }
    }
}

#[cfg(windows)]
pub fn claim_app_id() {
    platform::claim_app_id();
}

pub fn prepare(core: &Arc<Core>) {
    platform::bind(core);
}

pub fn new_mail(core: &Arc<Core>, mail: &NewMail) {
    platform::post(core, mail);
}

pub async fn status() -> NotifyStatus {
    platform::status().await
}

pub fn take_opened() -> Option<NotificationTarget> {
    LAST_OPENED.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::toast_xml;

    #[test]
    fn mail_text_cannot_break_out_of_the_toast_markup() {
        let xml = toast_xml(
            "Eve <eve@x.org>",
            "a & b",
            "\"quoted\" 'single' </text><action/>\u{0007}",
        );
        assert!(xml.contains("<text>Eve &lt;eve@x.org&gt;</text>"));
        assert!(xml.contains("<text>a &amp; b</text>"));
        assert!(
            xml.contains(
                "&quot;quoted&quot; &apos;single&apos; &lt;/text&gt;&lt;action/&gt;</text>"
            )
        );
        assert!(!xml.contains('\u{0007}'));
        assert_eq!(xml.matches("<text>").count(), 3);
    }

    #[test]
    fn an_empty_snippet_leaves_out_the_body_line() {
        assert_eq!(
            toast_xml("From", "Subject", "").matches("<text>").count(),
            2
        );
    }
}
