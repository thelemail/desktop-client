use std::borrow::Cow;

use gpui_kit::App;

const FONTS: [&[u8]; 14] = [
    include_bytes!("../assets/fonts/HankenGrotesk-Thin.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-ExtraLight.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Light.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-SemiBold.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Bold.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-ExtraBold.ttf"),
    include_bytes!("../assets/fonts/HankenGrotesk-Black.ttf"),
    include_bytes!("../assets/fonts/RedHatMono-Light.ttf"),
    include_bytes!("../assets/fonts/RedHatMono-Regular.ttf"),
    include_bytes!("../assets/fonts/RedHatMono-Medium.ttf"),
    include_bytes!("../assets/fonts/RedHatMono-SemiBold.ttf"),
    include_bytes!("../assets/fonts/RedHatMono-Bold.ttf"),
];

pub fn register(cx: &App) {
    let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::error!("fonts: registering the embedded fonts failed: {err}");
    }
}
