use std::ops::Range;

use gpui_kit::{
    App, ElementId, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString,
    StyledText, Window,
};
use thelemail_i18n::Rich;

use crate::theme::Theme;
use crate::tokens::{ColorToken, FONT_MONO};

const LINKS: [&str; 5] = ["link", "a", "retry", "terms", "privacy"];
const MONO: [&str; 3] = ["mono", "code", "addr"];

pub struct Spans {
    text: SharedString,
    tagged: Vec<(Range<usize>, &'static str)>,
}

impl Spans {
    pub fn new(rich: &Rich) -> Self {
        let mut text = String::new();
        let mut tagged = Vec::new();
        for run in rich.runs() {
            let start = text.len();
            text.push_str(&run.text);
            if let Some(tag) = run.tag
                && !run.text.is_empty()
            {
                tagged.push((start..text.len(), tag));
            }
        }
        Self {
            text: text.into(),
            tagged,
        }
    }

    pub fn styled(&self, theme: &Theme) -> StyledText {
        let highlights = self
            .tagged
            .iter()
            .filter_map(|(range, tag)| style(tag, theme).map(|style| (range.clone(), style)));
        let families = self
            .tagged
            .iter()
            .filter(|(_, tag)| MONO.contains(tag))
            .map(|(range, _)| (range.clone(), SharedString::from(FONT_MONO.name)));
        StyledText::new(self.text.clone())
            .with_highlights(highlights)
            .with_font_family_overrides(families)
    }

    pub fn interactive(
        &self,
        id: impl Into<ElementId>,
        theme: &Theme,
        on_link: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    ) -> InteractiveText {
        let links: Vec<_> = self
            .tagged
            .iter()
            .filter(|(_, tag)| LINKS.contains(tag))
            .cloned()
            .collect();
        let ranges = links.iter().map(|(range, _)| range.clone()).collect();
        InteractiveText::new(id, self.styled(theme)).on_click(ranges, move |index, window, cx| {
            if let Some((_, tag)) = links.get(index) {
                on_link(tag, window, cx);
            }
        })
    }
}

fn style(tag: &str, theme: &Theme) -> Option<HighlightStyle> {
    match tag {
        "b" | "strong" => Some(HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        }),
        "i" => Some(HighlightStyle {
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        }),
        tag if LINKS.contains(&tag) => Some(HighlightStyle {
            color: Some(theme.color(ColorToken::Link)),
            ..Default::default()
        }),
        _ => None,
    }
}
