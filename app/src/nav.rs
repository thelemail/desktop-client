use gpui_kit::{App, AppContext, Context, Entity, Global};
use thelemail_core::notify::NotificationTarget;

use crate::router::{AccountPage, History, Route};

pub struct Navigator {
    history: History,
    accounts: Vec<String>,
}

#[derive(Clone)]
pub struct Nav(pub Entity<Navigator>);

impl Global for Nav {}

impl Navigator {
    pub fn current(&self) -> &Route {
        self.history.current()
    }

    pub fn current_slot(&self) -> u8 {
        self.history.current().slot().unwrap_or(0)
    }

    pub fn push(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.history.push(route) {
            self.changed(cx);
        }
    }

    pub fn open_account_page(&mut self, page: AccountPage, cx: &mut Context<Self>) {
        let slot = self.current_slot();
        self.push(Route::Account { slot, page }, cx);
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if self.history.back() {
            self.changed(cx);
        }
    }

    pub fn forward(&mut self, cx: &mut Context<Self>) {
        if self.history.forward() {
            self.changed(cx);
        }
    }

    pub fn set_accounts(&mut self, accounts: Vec<String>) {
        self.accounts = accounts;
    }

    pub fn open_target(&mut self, target: &NotificationTarget, cx: &mut Context<Self>) -> bool {
        match Route::for_notification(target, &self.accounts) {
            Some(route) => {
                self.push(route, cx);
                true
            }
            None => false,
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }
}

pub fn init(start: Route, cx: &mut App) -> Entity<Navigator> {
    let nav = cx.new(|_| Navigator {
        history: History::new(start),
        accounts: Vec::new(),
    });
    cx.set_global(Nav(nav.clone()));
    nav
}

pub fn navigator(cx: &App) -> Entity<Navigator> {
    cx.global::<Nav>().0.clone()
}
