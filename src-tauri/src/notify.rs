use tauri::AppHandle;

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

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::OnceLock;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{AnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSDictionary, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use tauri::{AppHandle, Emitter, Manager};

    use super::NewMail;

    static AUTHORIZED: OnceLock<()> = OnceLock::new();
    static DELEGATE: OnceLock<Retained<TapDelegate>> = OnceLock::new();

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
                    let _ = app.emit(
                        "notification://opened",
                        serde_json::json!({ "accountId": account_id, "messageId": message_id }),
                    );
                }
                handler.call(());
            }
        }
    );

    fn bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn center() -> Retained<UNUserNotificationCenter> {
        UNUserNotificationCenter::currentNotificationCenter()
    }

    pub fn bind(app: &AppHandle) {
        if !bundled() {
            eprintln!("notifications: not running from an app bundle, notifications are disabled");
            return;
        }
        let _ = DELEGATE.get_or_init(|| {
            let this = TapDelegate::alloc().set_ivars(DelegateState { app: app.clone() });
            let delegate: Retained<TapDelegate> = unsafe { msg_send![super(this), init] };
            center().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            delegate
        });
        request_authorization();
    }

    pub fn request_authorization() {
        if !bundled() {
            return;
        }
        AUTHORIZED.get_or_init(|| {
            let options = UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge;
            let handler = block2::RcBlock::new(
                |granted: objc2::runtime::Bool, _err: *mut objc2_foundation::NSError| {
                    if !granted.as_bool() {
                        eprintln!("notifications: the user declined authorization");
                    }
                },
            );
            center().requestAuthorizationWithOptions_completionHandler(options, &handler);
        });
    }

    pub fn post(mail: &NewMail) {
        if !bundled() {
            return;
        }
        request_authorization();
        unsafe {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&mail.title()));
            content.setSubtitle(&NSString::from_str(&mail.subtitle()));
            if !mail.snippet.is_empty() {
                content.setBody(&NSString::from_str(&mail.snippet));
            }

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
            center().addNotificationRequest_withCompletionHandler(&request, None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::NewMail;

    pub fn bind(app: &AppHandle) {
        let _ = DELEGATE.get_or_init(|| {
            let this = TapDelegate::alloc().set_ivars(DelegateState { app: app.clone() });
            let delegate: Retained<TapDelegate> = unsafe { msg_send![super(this), init] };
            center().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            delegate
        });
        request_authorization();
    }

    pub fn request_authorization() {}

    pub fn post(_mail: &NewMail) {}
}

pub fn prepare(app: &AppHandle) {
    platform::bind(app);
}

pub fn new_mail(_app: &AppHandle, mail: &NewMail) {
    platform::post(mail);
}
