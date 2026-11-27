use std::time::{Duration, Instant};

pub(crate) const TYPEAHEAD_WINDOW: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default)]
pub(crate) struct Typeahead {
    query: String,
    at: Option<Instant>,
}

impl Typeahead {
    pub(crate) fn push(&mut self, typed: &str, now: Instant) -> &str {
        if self
            .at
            .is_none_or(|at| now.saturating_duration_since(at) > TYPEAHEAD_WINDOW)
        {
            self.query.clear();
        }
        self.query.push_str(typed);
        self.at = Some(now);
        &self.query
    }

    pub(crate) fn is_empty(&self, now: Instant) -> bool {
        self.query.is_empty()
            || self
                .at
                .is_none_or(|at| now.saturating_duration_since(at) > TYPEAHEAD_WINDOW)
    }

    pub(crate) fn reset(&mut self) {
        self.query.clear();
        self.at = None;
    }
}

pub(crate) fn find(labels: &[Option<&str>], current: Option<usize>, query: &str) -> Option<usize> {
    let mut chars = query.chars();
    let first = chars.next()?;
    let repeated = query.chars().count() > 1 && chars.all(|c| c == first);
    let needle = if repeated {
        first.to_lowercase().collect::<String>()
    } else {
        query.to_lowercase()
    };
    let single = needle.chars().count() == 1;
    let len = labels.len();
    let start = current.unwrap_or(0);
    (0..len)
        .map(|offset| (start + offset) % len)
        .filter(|&ix| !(single && Some(ix) == current))
        .find(|&ix| {
            labels[ix].is_some_and(|label| label.trim_start().to_lowercase().starts_with(&needle))
        })
        .filter(|&ix| Some(ix) != current)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sideways {
    Open,
    Close,
}

pub(crate) fn sideways(right: bool, rtl: bool) -> Sideways {
    if right != rtl {
        Sideways::Open
    } else {
        Sideways::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: [Option<&str>; 6] = [
        Some("Rename"),
        None,
        Some("Move to…"),
        Some("Move up"),
        Some("Mark read"),
        Some("Delete"),
    ];

    #[test]
    fn typing_jumps_to_the_first_match() {
        assert_eq!(find(&LABELS, None, "d"), Some(5));
        assert_eq!(find(&LABELS, None, "mov"), Some(2));
        assert_eq!(find(&LABELS, None, "x"), None);
    }

    #[test]
    fn typing_more_keeps_the_current_match() {
        assert_eq!(find(&LABELS, Some(2), "move"), None);
        assert_eq!(find(&LABELS, Some(2), "move u"), Some(3));
    }

    #[test]
    fn repeating_a_letter_cycles_through_its_matches() {
        assert_eq!(find(&LABELS, Some(2), "m"), Some(3));
        assert_eq!(find(&LABELS, Some(3), "mm"), Some(4));
        assert_eq!(find(&LABELS, Some(4), "mmm"), Some(2));
    }

    #[test]
    fn disabled_rows_are_never_matched() {
        let labels = [Some("Archive"), None, Some("Apply")];
        assert_eq!(find(&labels, Some(0), "a"), Some(2));
    }

    #[test]
    fn the_query_resets_after_a_pause() {
        let start = Instant::now();
        let mut typed = Typeahead::default();
        assert_eq!(typed.push("m", start), "m");
        assert_eq!(typed.push("o", start + Duration::from_millis(200)), "mo");
        assert_eq!(typed.push("d", start + Duration::from_millis(800)), "d");
        assert!(!typed.is_empty(start + Duration::from_millis(900)));
        assert!(typed.is_empty(start + Duration::from_millis(1400)));
    }

    #[test]
    fn submenus_follow_reading_direction() {
        assert_eq!(sideways(true, false), Sideways::Open);
        assert_eq!(sideways(false, false), Sideways::Close);
        assert_eq!(sideways(false, true), Sideways::Open);
        assert_eq!(sideways(true, true), Sideways::Close);
    }
}
