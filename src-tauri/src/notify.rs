use std::sync::Mutex;

use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone)]
pub struct NewMail {
    pub account_id: String,
    pub message_id: String,
    pub sender: String,
    pub subject: String,
    pub snippet: String,
}

impl NewMail {
    fn title(&self) -> String {
        if self.sender.is_empty() {
            "New message".to_owned()
        } else {
            self.sender.clone()
        }
    }

    fn subtitle(&self) -> String {
        if self.subject.is_empty() {
            "(no subject)".to_owned()
        } else {
            self.subject.clone()
        }
    }
}

#[derive(Clone, Serialize)]
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

#[derive(Clone, Serialize)]
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

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSetting, UNNotificationSettings,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use tauri::{AppHandle, Emitter, Manager};

    use super::{NewMail, NotificationTarget, NotifyStatus, remember_opened};

    static DELEGATE: OnceLock<Retained<TapDelegate>> = OnceLock::new();
    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static WARNED_UNBUNDLED: AtomicBool = AtomicBool::new(false);
    static WARNED_DENIED: AtomicBool = AtomicBool::new(false);
    static STATUS: Mutex<Option<NotifyStatus>> = Mutex::new(None);

    pub struct DelegateState {
        app: AppHandle,
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
                let app = &self.ivars().app;
                let info = response.notification().request().content().userInfo();
                let read = |key: &str| -> Option<String> {
                    let value = info.valueForKey(&NSString::from_str(key))?;
                    let text: Retained<NSString> = unsafe { Retained::cast_unchecked(value) };
                    Some(text.to_string())
                };

                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                if let (Some(account_id), Some(message_id)) = (read("accountId"), read("messageId"))
                {
                    let target = NotificationTarget {
                        account_id,
                        message_id,
                    };
                    remember_opened(target.clone());
                    let _ = app.emit("notification://opened", target);
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

    fn publish(app: &AppHandle, status: NotifyStatus) {
        eprintln!(
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
        let _ = app.emit("notify://status", status);
    }

    fn with_settings(f: impl Fn(&UNNotificationSettings) + 'static) {
        let block = block2::RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            f(unsafe { settings.as_ref() });
        });
        center().getNotificationSettingsWithCompletionHandler(&block);
    }

    fn refresh_status(app: &AppHandle, last_error: Option<String>) {
        let app = app.clone();
        let last_error = last_error.or_else(remembered_error);
        with_settings(move |settings| publish(&app, snapshot(settings, last_error.clone())));
    }

    pub fn bind(app: &AppHandle) {
        if !bundled() {
            publish(app, NotifyStatus::unbundled());
            return;
        }
        let _ = DELEGATE.get_or_init(|| {
            let this = TapDelegate::alloc().set_ivars(DelegateState { app: app.clone() });
            let delegate: Retained<TapDelegate> = unsafe { msg_send![super(this), init] };
            center().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            delegate
        });
        refresh_status(app, None);
        request_authorization(app, None);
    }

    pub fn request_authorization(app: &AppHandle, then: Option<NewMail>) {
        if !bundled() {
            return;
        }
        if then.is_none() && REQUESTED.swap(true, Ordering::SeqCst) {
            return;
        }
        REQUESTED.store(true, Ordering::SeqCst);
        let app = app.clone();
        let options = UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge;
        let handler = block2::RcBlock::new(
            move |granted: objc2::runtime::Bool, err: *mut NSError| {
                let error = describe_error(err);
                if !granted.as_bool() {
                    eprintln!(
                        "notifications: authorization was not granted: {}",
                        error.as_deref().unwrap_or("declined by the user")
                    );
                }
                refresh_status(&app, error);
                if granted.as_bool()
                    && let Some(mail) = &then
                {
                    deliver(&app, mail);
                }
            },
        );
        center().requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    fn deliver(app: &AppHandle, mail: &NewMail) {
        unsafe {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&mail.title()));
            content.setSubtitle(&NSString::from_str(&mail.subtitle()));
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
            let app = app.clone();
            let handler = block2::RcBlock::new(move |err: *mut NSError| {
                if let Some(error) = describe_error(err) {
                    eprintln!("notifications: the request was rejected: {error}");
                    refresh_status(&app, Some(error));
                }
            });
            center().addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        }
    }

    pub fn post(app: &AppHandle, mail: &NewMail) {
        if !bundled() {
            if !WARNED_UNBUNDLED.swap(true, Ordering::SeqCst) {
                eprintln!("notifications: not running from an app bundle, notifications are disabled");
            }
            return;
        }
        let app = app.clone();
        let mail = mail.clone();
        with_settings(move |settings| {
            let status = settings.authorizationStatus();
            if allowed(status) {
                deliver(&app, &mail);
            } else if status == UNAuthorizationStatus::NotDetermined {
                request_authorization(&app, Some(mail.clone()));
            } else if !WARNED_DENIED.swap(true, Ordering::SeqCst) {
                refresh_status(&app, None);
            }
        });
    }

    pub async fn status(_app: &AppHandle) -> NotifyStatus {
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

#[cfg(not(target_os = "macos"))]
mod platform {
    use tauri::AppHandle;

    use super::{NewMail, NotifyStatus};

    pub fn bind(_app: &AppHandle) {}

    pub fn post(_app: &AppHandle, _mail: &NewMail) {}

    pub async fn status(_app: &AppHandle) -> NotifyStatus {
        NotifyStatus {
            supported: false,
            authorization: "unsupported".to_owned(),
            ..NotifyStatus::unbundled()
        }
    }
}

pub fn prepare(app: &AppHandle) {
    platform::bind(app);
}

pub fn new_mail(app: &AppHandle, mail: &NewMail) {
    platform::post(app, mail);
}

#[tauri::command]
pub async fn notify_status(app: AppHandle) -> NotifyStatus {
    platform::status(&app).await
}

#[tauri::command]
pub fn notify_take_opened() -> Option<NotificationTarget> {
    LAST_OPENED.lock().ok().and_then(|mut slot| slot.take())
}
