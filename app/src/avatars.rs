use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{App, ElementId, Global, Image, ImageFormat};
use thelemail_core::avatars::{AvatarImage, Lookup};
use thelemail_ui::avatar::{Avatar, Choice, Picture, Sender, choice, sender};

use crate::services::Services;

const KEPT: usize = 400;

#[derive(Default)]
struct Pictures {
    images: HashMap<u64, Arc<Image>>,
}

impl Global for Pictures {}

fn image(cx: &mut App, avatar: &AvatarImage) -> Arc<Image> {
    let pictures = cx.default_global::<Pictures>();
    if pictures.images.len() >= KEPT && !pictures.images.contains_key(&avatar.id) {
        pictures.images.clear();
    }
    pictures
        .images
        .entry(avatar.id)
        .or_insert_with(|| Arc::new(Image::from_bytes(ImageFormat::Png, avatar.png.to_vec())))
        .clone()
}

pub struct Message<'a> {
    pub account_id: &'a str,
    pub display: &'a str,
    pub address: &'a str,
    pub attested: Option<&'a str>,
    pub bimi_domain: &'a str,
}

pub fn identity(message: &Message) -> Sender {
    let forged = thelemail_core::sender::attested_mismatch(message.attested, message.address);
    sender(message.display, message.address, forged)
}

pub fn lookups(cx: &App, message: &Message, who: &Sender) -> (Lookup, Lookup) {
    if who.mismatch {
        return (Lookup::Ready(None), Lookup::Ready(None));
    }
    let avatars = cx.global::<Services>().core().avatars();
    let photo = avatars.person(message.account_id, message.address);
    let logo = match photo.image() {
        Some(_) => Lookup::Ready(None),
        None => avatars.bimi(message.account_id, message.bimi_domain),
    };
    (photo, logo)
}

pub fn sender_avatar(cx: &mut App, id: impl Into<ElementId>, message: &Message) -> Avatar {
    let who = identity(message);
    let (photo, logo) = lookups(cx, message, &who);
    let picture = match choice(
        who.mismatch,
        photo.image().is_some(),
        logo.image().is_some(),
    ) {
        Choice::Photo => photo.image().map(|i| Picture::Photo(image(cx, i))),
        Choice::Logo => logo.image().map(|i| Picture::Logo(image(cx, i))),
        Choice::Initials => None,
    };
    Avatar::of(id, &who).picture(picture).label(who.from)
}

pub fn own_avatar(
    cx: &mut App,
    id: impl Into<ElementId>,
    account_id: &str,
    avatar_path: Option<&str>,
    who: &Sender,
) -> Avatar {
    let lookup = match avatar_path {
        Some(path) => cx
            .global::<Services>()
            .core()
            .avatars()
            .account_avatar(account_id, path),
        None => Lookup::Ready(None),
    };
    let picture = lookup.image().map(|i| Picture::Photo(image(cx, i)));
    Avatar::of(id, who).picture(picture).label(who.from.clone())
}
