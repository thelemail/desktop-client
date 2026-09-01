use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub struct NewMail {
    pub sender: String,
    pub subject: String,
    pub snippet: String,
}

pub fn new_mail(app: &AppHandle, mail: &NewMail) {
    let title = if mail.sender.is_empty() {
        "New message".to_owned()
    } else {
        mail.sender.clone()
    };
    let subject = if mail.subject.is_empty() {
        "(no subject)"
    } else {
        &mail.subject
    };
    let body = if mail.snippet.is_empty() {
        subject.to_owned()
    } else {
        format!("{subject}\n{}", mail.snippet)
    };

    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        eprintln!("notification failed: {err}");
    }
}
