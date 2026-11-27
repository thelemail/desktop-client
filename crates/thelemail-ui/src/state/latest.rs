#[derive(Debug, Default, Clone)]
pub struct Latest {
    token: u64,
}

impl Latest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin(&mut self) -> u64 {
        self.token += 1;
        self.token
    }

    pub fn is_current(&self, token: u64) -> bool {
        token == self.token
    }

    pub fn invalidate(&mut self) {
        self.token += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_begin_is_current() {
        let mut latest = Latest::new();
        let first = latest.begin();
        assert!(latest.is_current(first));
        let second = latest.begin();
        assert!(!latest.is_current(first));
        assert!(latest.is_current(second));
    }

    #[test]
    fn invalidate_retires_the_running_token() {
        let mut latest = Latest::new();
        let token = latest.begin();
        latest.invalidate();
        assert!(!latest.is_current(token));
        let next = latest.begin();
        assert!(latest.is_current(next));
    }

    #[test]
    fn a_token_from_before_any_begin_is_never_current_after_one() {
        let mut latest = Latest::new();
        latest.begin();
        assert!(!latest.is_current(0));
    }
}
