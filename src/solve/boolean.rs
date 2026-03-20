use super::Compatibility;

pub fn booleans_compatible(a: bool, b: bool) -> Compatibility {
    if a == b {
        Compatibility::Compatible
    } else {
        Compatibility::Incompatible(format!(
            "{} contradicts {}",
            a, b
        ))
    }
}
