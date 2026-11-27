use super::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Loading,
    Error,
    Empty { refreshing: bool, stale: bool },
    Content { refreshing: bool, stale: bool },
}

impl Region {
    pub fn of<T, E>(resource: &Resource<T, E>, is_empty: impl FnOnce(&T) -> bool) -> Self {
        let (data, refreshing, stale) = match resource {
            Resource::NotLoaded | Resource::Loading => return Self::Loading,
            Resource::Failed { data: None, .. } => return Self::Error,
            Resource::Loaded(data) => (data, false, false),
            Resource::Refreshing(data) => (data, true, false),
            Resource::Failed {
                data: Some(data), ..
            } => (data, false, true),
        };
        if is_empty(data) {
            Self::Empty { refreshing, stale }
        } else {
            Self::Content { refreshing, stale }
        }
    }

    pub fn busy(self) -> bool {
        matches!(
            self,
            Self::Loading
                | Self::Empty {
                    refreshing: true,
                    ..
                }
                | Self::Content {
                    refreshing: true,
                    ..
                }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Resource<Vec<u32>>;

    fn region(r: &R) -> Region {
        Region::of(r, Vec::is_empty)
    }

    #[test]
    fn nothing_fetched_yet_is_loading_never_empty() {
        assert_eq!(region(&R::NotLoaded), Region::Loading);
        assert_eq!(region(&R::Loading), Region::Loading);
    }

    #[test]
    fn a_failure_with_no_data_is_an_error_never_empty() {
        let mut r = R::default();
        r.begin();
        r.fail("offline".into());
        assert_eq!(region(&r), Region::Error);
    }

    #[test]
    fn a_loaded_empty_list_is_the_empty_state() {
        let mut r = R::default();
        r.succeed(vec![]);
        assert_eq!(
            region(&r),
            Region::Empty {
                refreshing: false,
                stale: false
            }
        );
    }

    #[test]
    fn a_refresh_keeps_the_content_and_marks_it_busy() {
        let mut r = R::default();
        r.succeed(vec![1]);
        r.begin();
        let now = region(&r);
        assert_eq!(
            now,
            Region::Content {
                refreshing: true,
                stale: false
            }
        );
        assert!(now.busy());
    }

    #[test]
    fn a_failed_refresh_keeps_the_content_and_flags_it_stale() {
        let mut r = R::default();
        r.succeed(vec![1, 2]);
        r.begin();
        r.fail("timeout".into());
        assert_eq!(
            region(&r),
            Region::Content {
                refreshing: false,
                stale: true
            }
        );
    }

    #[test]
    fn a_failed_refresh_of_an_empty_list_stays_empty_not_error() {
        let mut r = R::default();
        r.succeed(vec![]);
        r.fail("timeout".into());
        assert_eq!(
            region(&r),
            Region::Empty {
                refreshing: false,
                stale: true
            }
        );
    }
}
