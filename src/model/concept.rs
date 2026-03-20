use std::fmt;

/// A named semantic concept that multiple files can assert about.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticConcept {
    pub id: String,
    pub display_name: String,
    pub category: ConceptCategory,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConceptCategory {
    RuntimeVersion,
    Port,
    StrictMode,
    BuildTool,
    PackageManager,
    Custom(String),
}

impl fmt::Display for SemanticConcept {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name)
    }
}

// Well-known concepts
impl SemanticConcept {
    pub fn node_version() -> Self {
        Self {
            id: "node-version".into(),
            display_name: "Node.js Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    pub fn python_version() -> Self {
        Self {
            id: "python-version".into(),
            display_name: "Python Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    pub fn go_version() -> Self {
        Self {
            id: "go-version".into(),
            display_name: "Go Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    pub fn java_version() -> Self {
        Self {
            id: "java-version".into(),
            display_name: "Java Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    pub fn app_port() -> Self {
        Self {
            id: "app-port".into(),
            display_name: "Application Port".into(),
            category: ConceptCategory::Port,
        }
    }

    pub fn ts_strict_mode() -> Self {
        Self {
            id: "ts-strict-mode".into(),
            display_name: "TypeScript Strict Mode".into(),
            category: ConceptCategory::StrictMode,
        }
    }

    pub fn node_package_manager() -> Self {
        Self {
            id: "node-pkg-manager".into(),
            display_name: "Node Package Manager".into(),
            category: ConceptCategory::PackageManager,
        }
    }

    pub fn dotnet_version() -> Self {
        Self {
            id: "dotnet-version".into(),
            display_name: ".NET Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }

    pub fn ruby_version() -> Self {
        Self {
            id: "ruby-version".into(),
            display_name: "Ruby Version".into(),
            category: ConceptCategory::RuntimeVersion,
        }
    }
}
