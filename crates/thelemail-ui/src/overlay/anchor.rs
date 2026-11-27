use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::{Align, Placement, Positioner};
use gpui_kit::{
    Anchor, AnyElement, App, Bounds, ElementId, IntoElement, ParentElement, Pixels, Point, Styled,
    Window, canvas, deferred, div, px,
};

use super::layer::{self, Clearance, Layer, clear_of};

const MARGIN: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Place {
    Side {
        trigger: Bounds<Pixels>,
        placement: Placement,
        align: Align,
        offset: Pixels,
    },
    At(Point<Pixels>),
}

impl Place {
    fn trigger(&self) -> Bounds<Pixels> {
        match *self {
            Self::Side { trigger, .. } => trigger,
            Self::At(point) => Bounds {
                origin: point,
                size: Default::default(),
            },
        }
    }
}

#[derive(Default)]
struct Moved {
    from: Cell<Option<Bounds<Pixels>>>,
    to: Cell<Option<Point<Pixels>>>,
    pending: Cell<bool>,
}

pub(crate) fn bounds_of(slot: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {}).size_full())
}

pub(crate) fn slot(
    id: &ElementId,
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Rc<Cell<Bounds<Pixels>>> {
    window
        .use_keyed_state(
            ElementId::NamedChild(id.clone().into(), name.into()),
            cx,
            |_, _| Rc::new(Cell::new(Bounds::default())),
        )
        .read(cx)
        .clone()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn anchored(
    id: &ElementId,
    layer: Layer,
    depth: usize,
    place: Place,
    on_bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
    child: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    position(id, layer, depth, place, on_bounds, true, child, window, cx)
}

pub(crate) fn anchored_passive(
    id: &ElementId,
    layer: Layer,
    depth: usize,
    place: Place,
    child: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    position(id, layer, depth, place, None, false, child, window, cx)
}

#[allow(clippy::too_many_arguments)]
fn position(
    id: &ElementId,
    layer: Layer,
    depth: usize,
    place: Place,
    on_bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
    occlude: bool,
    child: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let moved = window
        .use_keyed_state(
            ElementId::NamedChild(id.clone().into(), "moved".into()),
            cx,
            |_, _| Rc::new(Moved::default()),
        )
        .read(cx)
        .clone();
    let trigger = place.trigger();
    if moved.from.get() != Some(trigger) {
        moved.from.set(Some(trigger));
        moved.to.set(None);
    }
    let cover = layer::cover(id, window, cx);
    let surfaces = layer::surfaces(window, cx);
    let viewport = window.viewport_size();

    let positioner = match (moved.to.get(), place) {
        (Some(origin), _) => Positioner::corner(Anchor::TopLeft, origin),
        (
            None,
            Place::Side {
                trigger,
                placement,
                align,
                offset,
            },
        ) => Positioner::side(trigger)
            .placement(placement)
            .align(align)
            .offset(offset),
        (None, Place::At(point)) => Positioner::corner(Anchor::TopLeft, point),
    };
    let watch = moved.clone();
    let positioner = if occlude {
        positioner.occlude()
    } else {
        positioner
    };
    let positioner = positioner.margin(px(MARGIN)).on_position(move |resolved| {
        if watch.to.get().is_some() {
            return;
        }
        if let Clearance::Move(origin) = clear_of(resolved.bounds, trigger, &surfaces, viewport) {
            watch.to.set(Some(origin));
            watch.pending.set(true);
        }
    });

    let tracked = div().relative().child(child).child(
        div().absolute().top_0().left_0().size_full().child(
            canvas(
                move |bounds, window, cx| {
                    cover.set(bounds, window, cx);
                    if let Some(slot) = on_bounds.as_ref() {
                        slot.set(bounds);
                    }
                    if moved.pending.replace(false) {
                        layer::redraw(window, cx);
                    }
                },
                |_, _, _, _| {},
            )
            .size_full(),
        ),
    );

    deferred(positioner.child(tracked))
        .with_priority(layer.priority() + depth)
        .into_any_element()
}
