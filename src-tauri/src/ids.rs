use thelemail_store::db::is_valid_account_id;

pub fn account_id(raw: &str) -> Result<&str, String> {
    if is_valid_account_id(raw) {
        Ok(raw)
    } else {
        Err("not a valid account id".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::account_id;

    #[test]
    fn only_a_canonical_uuid_is_accepted() {
        assert!(account_id("11111111-2222-3333-4444-555555555555").is_ok());
        for hostile in [
            "../../../../etc/passwd",
            "/Users/someone/Library/Safari/History.db",
            "11111111-2222-3333-4444-55555555555A",
            "11111111222233334444555555555555",
            "",
            "..",
        ] {
            assert!(
                account_id(hostile).is_err(),
                "accepted a hostile account id: {hostile:?}"
            );
        }
    }
}
