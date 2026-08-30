//! Immutable startup environment data.

use std::collections::BTreeMap;

pub type EnvMap = BTreeMap<String, String>;

#[must_use]
pub fn merge_env(
    mut initial: EnvMap,
    overrides: impl IntoIterator<Item = (String, String)>,
) -> EnvMap {
    initial.extend(overrides);
    initial
}

#[must_use]
pub fn value<'a>(env: &'a EnvMap, key: &str) -> Option<&'a str> {
    env.get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_win_without_changing_the_input_map() {
        let initial = EnvMap::from([
            ("PORT".into(), "8080".into()),
            ("SERVER_AUTH_SECRET".into(), "kept-out-of-cli".into()),
        ]);
        let merged = merge_env(initial.clone(), [("PORT".into(), "9090".into())]);
        assert_eq!(value(&merged, "PORT"), Some("9090"));
        assert_eq!(value(&initial, "PORT"), Some("8080"));
        assert_eq!(
            value(&merged, "SERVER_AUTH_SECRET"),
            Some("kept-out-of-cli")
        );
    }
}
