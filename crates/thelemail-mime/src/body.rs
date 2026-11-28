use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlinePart {
    pub mime_type: String,
    pub data: Vec<u8>,
}

pub type InlineParts = BTreeMap<String, InlinePart>;
