use gpui_kit::{Corners, Pixels, Styled};

pub trait CornerRadii: Styled + Sized {
    fn corners(mut self, radii: Corners<Pixels>) -> Self {
        let style = &mut self.style().corner_radii;
        style.top_left = Some(radii.top_left.into());
        style.top_right = Some(radii.top_right.into());
        style.bottom_right = Some(radii.bottom_right.into());
        style.bottom_left = Some(radii.bottom_left.into());
        self
    }
}

impl<E: Styled> CornerRadii for E {}

pub const ALL: Corners<bool> = Corners {
    top_left: true,
    top_right: true,
    bottom_right: true,
    bottom_left: true,
};

pub fn pick(radius: Pixels, corners: Corners<bool>) -> Corners<Pixels> {
    let on = |set: bool| if set { radius } else { Pixels::ZERO };
    Corners {
        top_left: on(corners.top_left),
        top_right: on(corners.top_right),
        bottom_right: on(corners.bottom_right),
        bottom_left: on(corners.bottom_left),
    }
}
