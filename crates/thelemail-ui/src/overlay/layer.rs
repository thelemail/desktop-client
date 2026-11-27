use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, ElementId, Global, IntoElement, ParentElement,
    Pixels, Point, Size, Styled, Window, anchored, canvas, deferred, div, point, px,
};

use crate::tokens::{Z_DIALOG, Z_POPOVER, Z_TOAST};

pub const Z_TRANSITION: u32 = 200;
const CLEARANCE: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Transition,
    Dialog,
    Popover,
    Toast,
}

impl Layer {
    pub const fn z(self) -> u32 {
        match self {
            Self::Transition => Z_TRANSITION,
            Self::Dialog => Z_DIALOG,
            Self::Popover => Z_POPOVER,
            Self::Toast => Z_TOAST,
        }
    }

    pub const fn priority(self) -> usize {
        self.z() as usize
    }
}

pub fn host(layer: Layer, child: impl IntoElement) -> AnyElement {
    deferred(anchored().position(point(px(0.0), px(0.0))).child(child))
        .with_priority(layer.priority())
        .into_any_element()
}

pub fn above(layer: Layer, depth: usize, child: impl IntoElement) -> AnyElement {
    deferred(anchored().position(point(px(0.0), px(0.0))).child(child))
        .with_priority(layer.priority() + depth)
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Surface,
    Cover,
}

type Key = (AnyWindowHandle, Kind, ElementId);

#[derive(Default)]
struct Registry {
    rects: HashMap<Key, Bounds<Pixels>>,
}

impl Registry {
    fn of(&self, window: AnyWindowHandle, kind: Kind) -> impl Iterator<Item = Bounds<Pixels>> {
        self.rects
            .iter()
            .filter(move |((w, k, _), _)| *w == window && *k == kind)
            .map(|(_, bounds)| *bounds)
    }

    fn covered(&self, window: AnyWindowHandle) -> bool {
        let covers: Vec<_> = self.of(window, Kind::Cover).collect();
        self.of(window, Kind::Surface)
            .any(|surface| covers.iter().any(|cover| cover.intersects(&surface)))
    }
}

#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Registry>>);

impl Global for Shared {}

fn shared(cx: &mut App) -> Shared {
    cx.default_global::<Shared>().clone()
}

pub(crate) struct Entry {
    shared: Shared,
    key: Key,
}

impl Entry {
    fn new(shared: Shared, key: Key) -> Self {
        Self { shared, key }
    }

    pub(crate) fn set(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let mut registry = self.shared.0.borrow_mut();
        let before = registry.covered(self.key.0);
        let changed = registry.rects.insert(self.key.clone(), bounds) != Some(bounds);
        if changed && registry.covered(self.key.0) != before {
            redraw(window, cx);
        }
    }

    pub(crate) fn clear(&self, window: &mut Window, cx: &mut App) {
        let mut registry = self.shared.0.borrow_mut();
        let before = registry.covered(self.key.0);
        if registry.rects.remove(&self.key).is_some() && registry.covered(self.key.0) != before {
            redraw(window, cx);
        }
    }
}

pub(crate) fn redraw(window: &mut Window, cx: &mut App) {
    window.defer(cx, |window, _| window.refresh());
}

impl Drop for Entry {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.shared.0.try_borrow_mut() {
            registry.rects.remove(&self.key);
        }
    }
}

fn entry(kind: Kind, id: ElementId, window: &mut Window, cx: &mut App) -> Rc<Entry> {
    let handle = window.window_handle();
    let registry = shared(cx);
    let state_id = ElementId::NamedChild(
        id.clone().into(),
        match kind {
            Kind::Surface => "native-surface",
            Kind::Cover => "overlay-cover",
        }
        .into(),
    );
    window
        .use_keyed_state(state_id, cx, move |_, _| {
            Rc::new(Entry::new(registry, (handle, kind, id)))
        })
        .read(cx)
        .clone()
}

pub(crate) fn cover(id: &ElementId, window: &mut Window, cx: &mut App) -> Rc<Entry> {
    entry(Kind::Cover, id.clone(), window, cx)
}

pub(crate) fn tracker(entry: Rc<Entry>) -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().child(
        canvas(
            move |bounds, window, cx| entry.set(bounds, window, cx),
            |_, _, _, _| {},
        )
        .size_full(),
    )
}

pub fn native_surface(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> AnyElement {
    tracker(entry(Kind::Surface, id.into(), window, cx)).into_any_element()
}

pub fn covered(window: &Window, cx: &App) -> bool {
    cx.try_global::<Shared>()
        .is_some_and(|shared| shared.0.borrow().covered(window.window_handle()))
}

pub(crate) fn surfaces(window: &Window, cx: &App) -> Vec<Bounds<Pixels>> {
    cx.try_global::<Shared>()
        .map(|shared| {
            shared
                .0
                .borrow()
                .of(window.window_handle(), Kind::Surface)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Clearance {
    Clear,
    Move(Point<Pixels>),
    Blocked,
}

pub(crate) fn clear_of(
    popup: Bounds<Pixels>,
    trigger: Bounds<Pixels>,
    surfaces: &[Bounds<Pixels>],
    viewport: Size<Pixels>,
) -> Clearance {
    let hits = |b: &Bounds<Pixels>| surfaces.iter().any(|s| s.intersects(b));
    if !hits(&popup) {
        return Clearance::Clear;
    }
    let gap = px(CLEARANCE);
    let size = popup.size;
    let fits = |origin: Point<Pixels>| {
        origin.x >= px(0.0)
            && origin.y >= px(0.0)
            && origin.x + size.width <= viewport.width
            && origin.y + size.height <= viewport.height
    };
    let mut candidates = vec![
        point(popup.origin.x, trigger.bottom() + gap),
        point(popup.origin.x, trigger.top() - gap - size.height),
    ];
    for surface in surfaces.iter().filter(|s| s.intersects(&popup)) {
        candidates.push(point(surface.left() - gap - size.width, popup.origin.y));
        candidates.push(point(surface.right() + gap, popup.origin.y));
        candidates.push(point(popup.origin.x, surface.top() - gap - size.height));
        candidates.push(point(popup.origin.x, surface.bottom() + gap));
    }
    candidates
        .into_iter()
        .find(|origin| {
            let placed = Bounds {
                origin: *origin,
                size,
            };
            fits(*origin) && !hits(&placed) && !placed.intersects(&trigger)
        })
        .map_or(Clearance::Blocked, Clearance::Move)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::size;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn layers_keep_the_web_order() {
        assert!(Layer::Transition.priority() < Layer::Dialog.priority());
        assert!(Layer::Dialog.priority() < Layer::Popover.priority());
        assert!(Layer::Popover.priority() < Layer::Toast.priority());
        assert_eq!(Layer::Dialog.z(), 1100);
        assert_eq!(Layer::Toast.z(), 1300);
    }

    #[test]
    fn a_popup_clear_of_every_surface_stays_put() {
        let viewport = size(px(1200.0), px(800.0));
        let popup = rect(10.0, 40.0, 200.0, 120.0);
        let trigger = rect(10.0, 10.0, 80.0, 24.0);
        let reader = rect(400.0, 0.0, 800.0, 800.0);
        assert_eq!(
            clear_of(popup, trigger, &[reader], viewport),
            Clearance::Clear
        );
    }

    #[test]
    fn a_popup_over_the_reader_moves_beside_it() {
        let viewport = size(px(1200.0), px(800.0));
        let reader = rect(400.0, 100.0, 800.0, 700.0);
        let trigger = rect(300.0, 60.0, 120.0, 32.0);
        let popup = rect(300.0, 96.0, 220.0, 160.0);
        let Clearance::Move(moved) = clear_of(popup, trigger, &[reader], viewport) else {
            panic!("expected a clear spot");
        };
        let placed = Bounds {
            origin: moved,
            size: popup.size,
        };
        assert!(!placed.intersects(&reader));
        assert!(placed.right() <= viewport.width && placed.bottom() <= viewport.height);
    }

    #[test]
    fn a_popup_never_moves_over_its_own_trigger() {
        let viewport = size(px(1000.0), px(1000.0));
        let reader = rect(170.0, 700.0, 830.0, 220.0);
        let trigger = rect(20.0, 200.0, 100.0, 36.0);
        let popup = rect(20.0, 240.0, 236.0, 520.0);
        match clear_of(popup, trigger, &[reader], viewport) {
            Clearance::Move(origin) => {
                let placed = Bounds {
                    origin,
                    size: popup.size,
                };
                assert!(!placed.intersects(&trigger));
                assert!(!placed.intersects(&reader));
            }
            other => assert_eq!(other, Clearance::Blocked),
        }
    }

    #[test]
    fn no_spot_means_the_surface_must_hide() {
        let viewport = size(px(600.0), px(400.0));
        let reader = rect(0.0, 0.0, 600.0, 400.0);
        let popup = rect(100.0, 100.0, 200.0, 100.0);
        let trigger = rect(100.0, 60.0, 80.0, 30.0);
        assert_eq!(
            clear_of(popup, trigger, &[reader], viewport),
            Clearance::Blocked
        );
    }
}
