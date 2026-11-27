use url::Url;

#[cfg(not(windows))]
pub const NOTIFICATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
#[cfg(windows)]
pub const NOTIFICATION_SETTINGS_URL: &str = "ms-settings:notifications";

pub fn allowed_external(url: &str) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|_| "not a url".to_owned())?;
    if parsed.as_str() == NOTIFICATION_SETTINGS_URL {
        return Ok(parsed);
    }
    match parsed.scheme() {
        "https" | "http" | "mailto" => Ok(parsed),
        other => Err(format!("refusing to open a {other} url")),
    }
}

pub fn open_external(url: &str) -> Result<(), String> {
    let url = allowed_external(url)?;
    platform::open(url.as_str())
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    pub fn open(url: &str) -> Result<(), String> {
        let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("not a url")?;
        if NSWorkspace::sharedWorkspace().openURL(&url) {
            Ok(())
        } else {
            Err("the system could not open the url".to_owned())
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;

    const SHOW_NORMAL: i32 = 1;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn open(url: &str) -> Result<(), String> {
        let verb = wide("open");
        let target = wide(url);
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SHOW_NORMAL,
            )
        };
        if result as isize > 32 {
            Ok(())
        } else {
            Err(format!(
                "the system could not open the url ({})",
                result as isize
            ))
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub fn open(_url: &str) -> Result<(), String> {
        Err("opening urls is not supported here".to_owned())
    }
}

pub fn safe_filename(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .filter(|c| {
            !cfg!(windows)
                || !(c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    let trimmed = if cfg!(windows) {
        trimmed.trim_end_matches(['.', ' '])
    } else {
        trimmed
    };
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub fn save_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_mail_and_the_settings_page_open_outside_the_app() {
        assert!(allowed_external("https://thelemail.com/help").is_ok());
        assert!(allowed_external("http://example.com/").is_ok());
        assert!(allowed_external("mailto:ada@calloway.thelemail.test").is_ok());
        assert!(allowed_external(NOTIFICATION_SETTINGS_URL).is_ok());
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "x-apple.systempreferences:com.apple.preference.security",
            "ms-settings:windowsupdate",
            "smb://server/share",
            "not a url",
        ] {
            assert!(
                allowed_external(refused).is_err(),
                "{refused} must not open"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn characters_windows_refuses_in_a_filename_are_dropped() {
        assert_eq!(safe_filename("Q3: plan?.pdf"), "Q3 plan.pdf");
        assert_eq!(safe_filename("a<b>|c\"d*.txt"), "abcd.txt");
        assert_eq!(safe_filename("notes. . "), "notes");
        assert_eq!(safe_filename("::"), "attachment");
    }
}
