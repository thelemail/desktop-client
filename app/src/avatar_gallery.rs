use std::sync::Arc;

use gpui_kit::component::input::InputState;
use gpui_kit::{
    AnyElement, App, AppContext, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, WeakEntity, Window, div, px,
};
use serde_json::{Value, json};
use thelemail_api::ApiRequest;
use thelemail_core::avatars::Lookup;
use thelemail_core::{Core, CoreEvent};
use thelemail_keystore::{OpaqueFinishAuthArgs, OpaqueFinishAuthResponse, OpaqueStartAuthArgs};
use thelemail_ui::avatar::{Avatar, PALETTE, sender};
use thelemail_ui::button::{Button, Size, Variant};
use thelemail_ui::feedback::{Status, Tone};
use thelemail_ui::field::{PasswordField, TextField};
use thelemail_ui::theme::{ActiveTokens, Appearance, Theme};
use thelemail_ui::tokens::{ColorToken, RADIUS_LG};

use crate::avatars::{Message, identity, lookups, own_avatar, sender_avatar};
use crate::bridge::{Hub, HubEvent};
use crate::gallery::{caption, frame_in, section};
use crate::language::Language;
use crate::router::GalleryTab;
use crate::services::Services;

const SIZES: [f32; 5] = [22.0, 26.0, 30.0, 34.0, 42.0];
const SAMPLES: [(&str, &str); 6] = [
    ("Ada Lovelace", "ada@calloway.thelemail.test"),
    ("Zoe West", "zoe.west@calloway.thelemail.test"),
    ("李小龙", "li@calloway.thelemail.test"),
    ("Élodie Durand", "elodie@calloway.thelemail.test"),
    ("", "billing@calloway.thelemail.test"),
    ("", ""),
];
const FORGER: &str = "mallory@thelemail.com";

#[derive(Clone)]
struct Session {
    account_id: String,
    email: String,
    avatar_path: Option<String>,
}

pub struct AvatarGallery {
    email: Entity<InputState>,
    password: Entity<PasswordField>,
    address: Entity<InputState>,
    domain: Entity<InputState>,
    session: Option<Session>,
    busy: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl AvatarGallery {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let email = input("Email on the local stack", window, cx);
        let address = input("Sender address", window, cx);
        let domain = input("BIMI domain", window, cx);
        let password = cx.new(|cx| PasswordField::new("avatar-gallery-password", window, cx));
        let hub = cx.global::<Hub>().0.clone();
        let subscriptions = vec![
            cx.subscribe(&hub, |_, _, event: &HubEvent, cx| {
                if matches!(event, HubEvent::Core(CoreEvent::AvatarsChanged { .. })) {
                    cx.notify();
                }
            }),
            cx.observe(&address, |_, _, cx| cx.notify()),
            cx.observe(&domain, |_, _, cx| cx.notify()),
        ];
        Self {
            email,
            password,
            address,
            domain,
            session: None,
            busy: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = self.email.read(cx).value().trim().to_owned();
        let password = self.password.read(cx).state().read(cx).value().to_string();
        if email.is_empty() || password.is_empty() {
            self.error = Some("Enter the email and password of a local account.".into());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let core = cx.global::<Services>().core().clone();
        let (tx, rx) = async_channel::bounded(1);
        cx.global::<Services>().runtime().spawn(async move {
            let _ = tx.send(dev_sign_in(core, email, password).await).await;
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = rx.recv().await else { return };
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(session) => this.session = Some(session),
                    Err(error) => this.error = Some(error.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

async fn post(core: &Core, path: &str, body: Value, token: Option<&str>) -> Result<Value, String> {
    let mut url = core.net().config().api_base.clone();
    url.set_path(path);
    let mut headers = std::collections::HashMap::new();
    headers.insert("Content-Type".to_owned(), "application/json".to_owned());
    if let Some(token) = token {
        headers.insert("Authorization".to_owned(), format!("Bearer {token}"));
    }
    let resp = thelemail_core::net::api_request(
        core,
        ApiRequest {
            url: url.to_string(),
            method: if body.is_null() { "GET" } else { "POST" }.to_owned(),
            headers,
            body: (!body.is_null()).then(|| body.to_string()),
        },
    )
    .await
    .map_err(|e| format!("{path}: {e}"))?;
    if !(200..300).contains(&resp.status) {
        return Err(format!("{path} answered {}", resp.status));
    }
    Ok(resp
        .body
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null))
}

async fn dev_sign_in(core: Arc<Core>, email: String, password: String) -> Result<Session, String> {
    let start = core
        .keystore()
        .opaque_start_auth(OpaqueStartAuthArgs {
            password,
            email: Some(email.clone()),
            recovery: false,
        })
        .await
        .map_err(|e| e.to_string())?;
    let init = post(
        &core,
        "/v1/auth/login/init",
        json!({ "email": email, "ke1": start.ke1 }),
        None,
    )
    .await?;
    let account_id = init["accountId"]
        .as_str()
        .ok_or("no account id")?
        .to_owned();
    let finished = core
        .keystore()
        .opaque_finish_auth(OpaqueFinishAuthArgs {
            operation_id: start.operation_id,
            account_id: account_id.clone(),
            ke2: init["ke2"].as_str().ok_or("no ke2")?.to_owned(),
            recovery: false,
        })
        .await;
    let ke3 = match finished {
        OpaqueFinishAuthResponse::Ok { ke3, .. } => ke3,
        OpaqueFinishAuthResponse::Err { code, .. } => return Err(code.to_owned()),
    };
    let complete = post(
        &core,
        "/v1/auth/login/complete",
        json!({
            "challengeId": init["challengeId"],
            "ke3": ke3,
            "enrollPersistentSession": false
        }),
        None,
    )
    .await?;
    let grant = complete.get("grant").cloned().unwrap_or(complete);
    let token = grant["accessToken"]
        .as_str()
        .ok_or("no access token")?
        .to_owned();
    let me = post(&core, "/v1/me", Value::Null, Some(&token)).await?;
    let mirror = core.clone();
    let id = account_id.clone();
    tokio::task::spawn_blocking(move || mirror.mirror().open(&id))
        .await
        .map_err(|e| e.to_string())??;
    core.mirror().set_token(&account_id, &token);
    Ok(Session {
        account_id,
        email: me["email"].as_str().unwrap_or(&email).to_owned(),
        avatar_path: me["avatarPath"].as_str().map(str::to_owned),
    })
}

fn themes(current: &Theme) -> [Theme; 2] {
    [
        Theme {
            appearance: Appearance::Light,
            ..*current
        },
        Theme {
            appearance: Appearance::Dark,
            ..*current
        },
    ]
}

fn side(theme: &Theme) -> &'static str {
    match theme.appearance {
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    }
}

fn id(theme: &Theme, name: impl Into<SharedString>) -> ElementId {
    ElementId::Name(format!("{}-avatar-{}", side(theme), name.into()).into())
}

fn strip(theme: &Theme, body: impl IntoElement) -> gpui_kit::Div {
    div()
        .p(px(16.0))
        .rounded(px(RADIUS_LG))
        .bg(theme.color(ColorToken::Surface))
        .border_1()
        .border_color(theme.color(ColorToken::BorderSoft))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.0))
        .child(body)
}

fn describe(lookup: &Lookup) -> &'static str {
    match lookup {
        Lookup::Pending => "pending",
        Lookup::Ready(Some(_)) => "image",
        Lookup::Ready(None) => "none",
    }
}

impl Render for AvatarGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = *cx.tokens();
        let language = *cx.global::<Language>();
        let this = cx.entity().downgrade();
        let live = self.live(&current, &this, cx);
        let mut live = Some(live);
        frame_in(
            GalleryTab::Avatars,
            &current,
            language,
            &[current.appearance],
            |theme| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(28.0))
                    .child(section("Initials", initials_strips(theme), theme))
                    .child(section("Palette", palette_strips(theme), theme))
                    .children(live.take())
                    .into_any_element()
            },
        )
    }
}

fn initials_strips(current: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .children(themes(current).map(|theme| {
            strip(
                &theme,
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(14.0))
                    .children(
                        SAMPLES
                            .iter()
                            .enumerate()
                            .flat_map(|(i, (display, address))| {
                                let who = sender(display, address, None);
                                SIZES.iter().map(move |size| {
                                    Avatar::of(id(&theme, format!("sample-{i}-{size}")), &who)
                                        .size(*size)
                                        .label(who.from.clone())
                                        .theme(theme)
                                })
                            }),
                    ),
            )
        }))
        .into_any_element()
}

fn palette_strips(current: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .children(themes(current).map(|theme| {
            strip(
                &theme,
                div()
                    .flex()
                    .gap(px(14.0))
                    .children((0..PALETTE.len()).map(|slot| {
                        Avatar::new(id(&theme, format!("slot-{slot}")), "TH", slot)
                            .size(34.0)
                            .theme(theme)
                    })),
            )
        }))
        .into_any_element()
}

impl AvatarGallery {
    fn live(&self, current: &Theme, this: &WeakEntity<Self>, cx: &mut App) -> AnyElement {
        let Some(session) = self.session.clone() else {
            return section("Live lookups", self.form(current, this), current).into_any_element();
        };
        let address = self.address.read(cx).value().trim().to_owned();
        let domain = self.domain.read(cx).value().trim().to_owned();
        let looked_up = if address.is_empty() {
            session.email.clone()
        } else {
            address
        };
        let probe_domain = if domain.is_empty() {
            looked_up.rsplit('@').next().unwrap_or_default().to_owned()
        } else {
            domain
        };
        let genuine = Message {
            account_id: &session.account_id,
            display: "",
            address: &looked_up,
            attested: None,
            bimi_domain: &probe_domain,
        };
        let forged = Message {
            attested: Some(FORGER),
            ..genuine
        };
        let who = identity(&genuine);
        let (photo, logo) = lookups(cx, &genuine, &who);
        let state = format!(
            "{looked_up}: photo {}, logo for {probe_domain} {}",
            describe(&photo),
            describe(&logo)
        );
        let strips: Vec<AnyElement> = themes(current)
            .into_iter()
            .map(|theme| {
                let own = sender("", &session.email, None);
                strip(
                    &theme,
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(
                            own_avatar(
                                cx,
                                id(&theme, "own"),
                                &session.account_id,
                                session.avatar_path.as_deref(),
                                &own,
                            )
                            .size(42.0)
                            .theme(theme),
                        )
                        .child(
                            sender_avatar(cx, id(&theme, "genuine"), &genuine)
                                .size(42.0)
                                .theme(theme),
                        )
                        .child(
                            sender_avatar(cx, id(&theme, "forged"), &forged)
                                .size(42.0)
                                .theme(theme),
                        )
                        .child(
                            sender_avatar(cx, id(&theme, "row"), &genuine)
                                .size(22.0)
                                .font(9.0)
                                .theme(theme),
                        ),
                )
                .into_any_element()
            })
            .collect();
        let forget = this.clone();
        let account = session.account_id.clone();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(caption(
                format!(
                    "signed in as {}, own avatar {}",
                    session.email,
                    session.avatar_path.as_deref().unwrap_or("not set")
                ),
                current,
            ))
            .child(
                div()
                    .w(px(360.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(TextField::new("avatar-gallery-address", &self.address).theme(*current))
                    .child(TextField::new("avatar-gallery-domain", &self.domain).theme(*current)),
            )
            .children(strips)
            .child(caption(
                format!("own, looked up, forged by {FORGER}, list row. {state}"),
                current,
            ))
            .child(
                Button::new("avatar-gallery-forget")
                    .size(Size::Sm)
                    .variant(Variant::Neutral)
                    .label("Drop memory cache")
                    .theme(*current)
                    .on_click(move |_, _, cx| {
                        cx.global::<Services>().core().avatars().forget(&account);
                        forget.update(cx, |_, cx| cx.notify()).ok();
                    }),
            );
        section("Live lookups", body.into_any_element(), current).into_any_element()
    }

    fn form(&self, theme: &Theme, this: &WeakEntity<Self>) -> AnyElement {
        let this = this.clone();
        div()
            .w(px(360.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(caption(
                "Sign in to a local account to fetch real photos and logos".to_owned(),
                theme,
            ))
            .child(TextField::new("avatar-gallery-email", &self.email).theme(*theme))
            .child(self.password.clone())
            .child(
                Button::new("avatar-gallery-sign-in")
                    .primary()
                    .label("Sign in")
                    .busy(self.busy)
                    .theme(*theme)
                    .on_click(move |_, window, cx| {
                        this.update(cx, |this, cx| this.sign_in(window, cx)).ok();
                    }),
            )
            .child(
                Status::new("avatar-gallery-status")
                    .tone(if self.error.is_some() {
                        Tone::Bad
                    } else {
                        Tone::Hint
                    })
                    .text(self.error.clone().unwrap_or_default())
                    .theme(*theme),
            )
            .into_any_element()
    }
}
