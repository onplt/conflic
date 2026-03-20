use crate::model::{Authority, Severity};

/// Compute severity based on the authority of both assertions.
pub fn compute_severity(left: Authority, right: Authority) -> Severity {
    let (higher, lower) = if left >= right {
        (left, right)
    } else {
        (right, left)
    };

    match (higher, lower) {
        (Authority::Enforced, Authority::Enforced) => Severity::Error,
        (Authority::Enforced, Authority::Declared) => Severity::Error,
        (Authority::Enforced, Authority::Advisory) => Severity::Warning,
        (Authority::Declared, Authority::Declared) => Severity::Warning,
        (Authority::Declared, Authority::Advisory) => Severity::Info,
        (Authority::Advisory, Authority::Advisory) => Severity::Info,
        // These cases shouldn't occur due to the ordering above, but handle them
        _ => Severity::Warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforced_enforced() {
        assert_eq!(
            compute_severity(Authority::Enforced, Authority::Enforced),
            Severity::Error
        );
    }

    #[test]
    fn test_enforced_declared() {
        assert_eq!(
            compute_severity(Authority::Enforced, Authority::Declared),
            Severity::Error
        );
    }

    #[test]
    fn test_enforced_advisory() {
        assert_eq!(
            compute_severity(Authority::Enforced, Authority::Advisory),
            Severity::Warning
        );
    }

    #[test]
    fn test_declared_declared() {
        assert_eq!(
            compute_severity(Authority::Declared, Authority::Declared),
            Severity::Warning
        );
    }

    #[test]
    fn test_declared_advisory() {
        assert_eq!(
            compute_severity(Authority::Declared, Authority::Advisory),
            Severity::Info
        );
    }

    #[test]
    fn test_advisory_advisory() {
        assert_eq!(
            compute_severity(Authority::Advisory, Authority::Advisory),
            Severity::Info
        );
    }
}
