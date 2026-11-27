use std::mem;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Resource<T, E = String> {
    #[default]
    NotLoaded,
    Loading,
    Loaded(T),
    Refreshing(T),
    Failed {
        error: E,
        data: Option<T>,
    },
}

impl<T, E> Resource<T, E> {
    pub fn begin(&mut self) {
        *self = match self.take() {
            Self::Loaded(data) | Self::Refreshing(data) => Self::Refreshing(data),
            Self::Failed {
                data: Some(data), ..
            } => Self::Refreshing(data),
            Self::NotLoaded | Self::Loading | Self::Failed { data: None, .. } => Self::Loading,
        };
    }

    pub fn succeed(&mut self, data: T) {
        *self = Self::Loaded(data);
    }

    pub fn fail(&mut self, error: E) {
        let data = match self.take() {
            Self::Loaded(data) | Self::Refreshing(data) => Some(data),
            Self::Failed { data, .. } => data,
            Self::NotLoaded | Self::Loading => None,
        };
        *self = Self::Failed { error, data };
    }

    pub fn reset(&mut self) {
        *self = Self::NotLoaded;
    }

    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Loaded(data) | Self::Refreshing(data) => Some(data),
            Self::Failed { data, .. } => data.as_ref(),
            Self::NotLoaded | Self::Loading => None,
        }
    }

    pub fn error(&self) -> Option<&E> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Loading | Self::Refreshing(_))
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_) | Self::Refreshing(_))
    }

    fn take(&mut self) -> Self {
        mem::replace(self, Self::NotLoaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Resource<Vec<u32>>;

    #[test]
    fn a_first_load_goes_through_loading_not_empty() {
        let mut r = R::default();
        assert_eq!(r, R::NotLoaded);
        assert!(r.data().is_none());
        r.begin();
        assert_eq!(r, R::Loading);
        assert!(r.is_busy());
        assert!(!r.is_loaded());
        r.succeed(vec![]);
        assert_eq!(r, R::Loaded(vec![]));
        assert!(r.is_loaded());
    }

    #[test]
    fn reloading_keeps_the_data_on_screen() {
        let mut r = R::Loaded(vec![1]);
        r.begin();
        assert_eq!(r, R::Refreshing(vec![1]));
        assert_eq!(r.data(), Some(&vec![1]));
        assert!(r.is_busy());
        r.succeed(vec![1, 2]);
        assert_eq!(r, R::Loaded(vec![1, 2]));
    }

    #[test]
    fn a_failed_refresh_keeps_the_data_and_reports_the_error() {
        let mut r = R::Loaded(vec![7]);
        r.begin();
        r.fail("offline".into());
        assert_eq!(r.data(), Some(&vec![7]));
        assert_eq!(r.error().map(String::as_str), Some("offline"));
        assert!(!r.is_loaded());
        assert!(!r.is_busy());
    }

    #[test]
    fn a_failed_first_load_has_no_data_and_is_never_loaded() {
        let mut r = R::default();
        r.begin();
        r.fail("boom".into());
        assert!(r.data().is_none());
        assert!(!r.is_loaded());
        assert_eq!(
            r,
            R::Failed {
                error: "boom".into(),
                data: None
            }
        );
    }

    #[test]
    fn retrying_after_a_failure_resumes_from_what_was_kept() {
        let mut with_data = R::Failed {
            error: "x".into(),
            data: Some(vec![3]),
        };
        with_data.begin();
        assert_eq!(with_data, R::Refreshing(vec![3]));

        let mut without = R::Failed {
            error: "x".into(),
            data: None,
        };
        without.begin();
        assert_eq!(without, R::Loading);
    }

    #[test]
    fn a_second_failure_keeps_data_from_before_the_first() {
        let mut r = R::Failed {
            error: "a".into(),
            data: Some(vec![9]),
        };
        r.fail("b".into());
        assert_eq!(r.data(), Some(&vec![9]));
        assert_eq!(r.error().map(String::as_str), Some("b"));
    }

    #[test]
    fn beginning_twice_stays_loading() {
        let mut r = R::default();
        r.begin();
        r.begin();
        assert_eq!(r, R::Loading);
    }

    #[test]
    fn reset_forgets_everything() {
        let mut r = R::Loaded(vec![1]);
        r.reset();
        assert_eq!(r, R::NotLoaded);
    }
}
