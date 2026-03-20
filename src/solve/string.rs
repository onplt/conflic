use super::Compatibility;

pub fn strings_compatible(a: &str, b: &str) -> Compatibility {
    if a == b {
        Compatibility::Compatible
    } else {
        Compatibility::Incompatible(format!(
            "\"{}\" differs from \"{}\"",
            a, b
        ))
    }
}
