use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KernelArgs(BTreeMap<String, Option<String>>);

impl KernelArgs {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn insert_flag(&mut self, key: impl Into<String>) {
        self.0.insert(key.into(), None);
    }

    pub fn insert_value(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), Some(value.into()));
    }
}

impl std::fmt::Display for KernelArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let rendered = self
            .0
            .iter()
            .map(|(key, value)| match value {
                Some(value) => format!("{key}={value}"),
                None => key.clone(),
            })
            .collect::<Vec<_>>()
            .join(" ");
        write!(f, "{rendered}")
    }
}

impl Deref for KernelArgs {
    type Target = BTreeMap<String, Option<String>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for KernelArgs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const N: usize> From<[(String, Option<String>); N]> for KernelArgs {
    fn from(value: [(String, Option<String>); N]) -> Self {
        Self(BTreeMap::from(value))
    }
}

impl FromIterator<(String, Option<String>)> for KernelArgs {
    fn from_iter<T: IntoIterator<Item = (String, Option<String>)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

pub fn parse_kernel_args(raw: impl AsRef<str>) -> KernelArgs {
    let mut parsed = KernelArgs::new();

    for pair in raw.as_ref().split_whitespace() {
        let mut segments = pair.splitn(2, '=');
        let key = segments.next().unwrap_or_default();
        let value = segments.next();
        parsed
            .0
            .insert(key.to_string(), value.map(ToOwned::to_owned));
    }

    parsed
}
