//! Knowledge schema validator for circle knowledge conventions.
//!
//! Validates page content against a [`KnowledgeSchema`], producing
//! [`SchemaViolation`]s for any rules that are not satisfied.

use std::collections::HashMap;

use sera_types::skill::{EnforcementMode, KnowledgeSchema, PageTypeRule};

/// Severity of a schema violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationSeverity {
    /// The page must be corrected before the write is accepted (enforced mode).
    Error,
    /// The page deviates from conventions but the write is still accepted.
    Warning,
}

/// A single rule violation found during schema validation.
#[derive(Debug, Clone)]
pub struct SchemaViolation {
    /// The rule that was violated (e.g., "naming_pattern", "required_field").
    pub rule: String,
    /// Human-readable description of what went wrong.
    pub message: String,
    /// How severe the violation is.
    pub severity: ViolationSeverity,
}

/// Validates page content against a [`KnowledgeSchema`].
#[derive(Debug, Default)]
pub struct KnowledgeSchemaValidator;

impl KnowledgeSchemaValidator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self
    }

    /// Validate a page name against the naming pattern for the given page type.
    ///
    /// Returns a list of violations. An empty list means the name is conformant.
    pub fn validate_page_name(
        &self,
        name: &str,
        page_type: &str,
        schema: &KnowledgeSchema,
    ) -> Vec<SchemaViolation> {
        let Some(rule) = find_page_type(schema, page_type) else {
            return vec![SchemaViolation {
                rule: "unknown_page_type".to_string(),
                message: format!("Page type '{page_type}' is not defined in schema '{}'", schema.name),
                severity: severity_for(schema),
            }];
        };

        if !matches_naming_pattern(name, &rule.naming_pattern) {
            vec![SchemaViolation {
                rule: "naming_pattern".to_string(),
                message: format!(
                    "Page name '{name}' does not match the required pattern '{}' for type '{page_type}'",
                    rule.naming_pattern
                ),
                severity: severity_for(schema),
            }]
        } else {
            vec![]
        }
    }

    /// Validate that all required frontmatter fields are present.
    ///
    /// Returns a list of violations for each missing required field.
    pub fn validate_required_fields(
        &self,
        frontmatter: &HashMap<String, String>,
        page_type: &str,
        schema: &KnowledgeSchema,
    ) -> Vec<SchemaViolation> {
        let Some(rule) = find_page_type(schema, page_type) else {
            return vec![SchemaViolation {
                rule: "unknown_page_type".to_string(),
                message: format!("Page type '{page_type}' is not defined in schema '{}'", schema.name),
                severity: severity_for(schema),
            }];
        };

        rule.required_fields
            .iter()
            .filter(|field| !frontmatter.contains_key(field.as_str()))
            .map(|field| SchemaViolation {
                rule: "required_field".to_string(),
                message: format!(
                    "Required frontmatter field '{field}' is missing for page type '{page_type}'"
                ),
                severity: severity_for(schema),
            })
            .collect()
    }

    /// Validate that all required cross-references are satisfied.
    ///
    /// `references` is a list of page-type strings that this page already links to.
    /// Returns violations for missing required cross-references.
    pub fn validate_cross_references(
        &self,
        page_type: &str,
        references: &[String],
        schema: &KnowledgeSchema,
    ) -> Vec<SchemaViolation> {
        schema
            .cross_reference_rules
            .iter()
            .filter(|r| r.from_type == page_type && r.required)
            .filter(|r| !references.iter().any(|ref_type| ref_type == &r.to_type))
            .map(|r| SchemaViolation {
                rule: "cross_reference".to_string(),
                message: format!(
                    "Page type '{}' must reference at least one page of type '{}', but none found",
                    r.from_type, r.to_type
                ),
                severity: severity_for(schema),
            })
            .collect()
    }
}

// --- helpers ---

fn find_page_type<'a>(schema: &'a KnowledgeSchema, page_type: &str) -> Option<&'a PageTypeRule> {
    schema.page_types.iter().find(|r| r.name == page_type)
}

/// Map the schema's enforcement mode to a violation severity.
fn severity_for(schema: &KnowledgeSchema) -> ViolationSeverity {
    match schema.enforcement_mode {
        EnforcementMode::Enforced => ViolationSeverity::Error,
        EnforcementMode::Advisory => ViolationSeverity::Warning,
    }
}

/// Minimal naming-pattern matcher.
///
/// The pattern language is intentionally simple:
/// - `YYYY` matches exactly 4 ASCII digits
/// - `MM` matches exactly 2 ASCII digits
/// - `DD` matches exactly 2 ASCII digits
/// - `<slug>` matches one or more lowercase-alphanumeric or hyphen characters
/// - Any other literal characters must appear verbatim
///
/// Example: `"YYYY-MM-DD-<slug>"` matches `"2024-03-15-my-decision"`.
fn matches_naming_pattern(name: &str, pattern: &str) -> bool {
    matches_pattern_recursive(name, pattern)
}

type TokenMatcher = (&'static str, fn(&str) -> Option<usize>);

fn matches_pattern_recursive(input: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return input.is_empty();
    }

    // Try each known token first
    let tokens: &[TokenMatcher] = &[
        ("YYYY", |s| {
            if s.len() >= 4 && s[..4].chars().all(|c| c.is_ascii_digit()) {
                Some(4)
            } else {
                None
            }
        }),
        ("MM", |s| {
            if s.len() >= 2 && s[..2].chars().all(|c| c.is_ascii_digit()) {
                Some(2)
            } else {
                None
            }
        }),
        ("DD", |s| {
            if s.len() >= 2 && s[..2].chars().all(|c| c.is_ascii_digit()) {
                Some(2)
            } else {
                None
            }
        }),
        ("<slug>", |s| {
            let len = s
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .count();
            if len > 0 { Some(len) } else { None }
        }),
    ];

    for (token, matcher) in tokens {
        if let Some(pat_rest) = pattern.strip_prefix(token)
            && let Some(consumed) = matcher(input)
            && matches_pattern_recursive(&input[consumed..], pat_rest)
        {
            return true;
        }
    }

    // Literal character match
    let pat_char = pattern.chars().next().unwrap();
    let pat_char_len = pat_char.len_utf8();
    if let Some(inp_char) = input.chars().next()
        && inp_char == pat_char
    {
        return matches_pattern_recursive(&input[inp_char.len_utf8()..], &pattern[pat_char_len..]);
    }

    false
}

/// Returns a sensible default schema for the system circle.
///
/// Includes common page types (`decision`, `architecture`, `runbook`),
/// categories, and a cross-reference rule requiring decisions to link
/// to at least one architecture page.
pub fn default_schema() -> KnowledgeSchema {
    use sera_types::skill::{CategoryRule, CrossReferenceRule, PageTypeRule};

    KnowledgeSchema {
        name: "system-circle-default".to_string(),
        version: "1.0.0".to_string(),
        enforcement_mode: EnforcementMode::Advisory,
        page_types: vec![
            PageTypeRule {
                name: "decision".to_string(),
                naming_pattern: "YYYY-MM-DD-<slug>".to_string(),
                required_fields: vec!["title".to_string(), "status".to_string(), "date".to_string()],
                description: Some("Architecture decision records (ADRs)".to_string()),
            },
            PageTypeRule {
                name: "architecture".to_string(),
                naming_pattern: "<slug>".to_string(),
                required_fields: vec!["title".to_string(), "owner".to_string()],
                description: Some("System architecture documentation".to_string()),
            },
            PageTypeRule {
                name: "runbook".to_string(),
                naming_pattern: "<slug>".to_string(),
                required_fields: vec!["title".to_string(), "service".to_string()],
                description: Some("Operational runbooks for on-call engineers".to_string()),
            },
        ],
        categories: vec![
            CategoryRule {
                name: "design".to_string(),
                allowed_page_types: vec!["decision".to_string(), "architecture".to_string()],
                description: Some("Design and architectural knowledge".to_string()),
            },
            CategoryRule {
                name: "operations".to_string(),
                allowed_page_types: vec!["runbook".to_string()],
                description: Some("Operational procedures".to_string()),
            },
        ],
        cross_reference_rules: vec![CrossReferenceRule {
            from_type: "decision".to_string(),
            to_type: "architecture".to_string(),
            required: true,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::skill::{CategoryRule, CrossReferenceRule, KnowledgeSchema, PageTypeRule};

    fn make_enforced_schema() -> KnowledgeSchema {
        KnowledgeSchema {
            name: "test-schema".to_string(),
            version: "1.0.0".to_string(),
            enforcement_mode: EnforcementMode::Enforced,
            page_types: vec![
                PageTypeRule {
                    name: "decision".to_string(),
                    naming_pattern: "YYYY-MM-DD-<slug>".to_string(),
                    required_fields: vec!["title".to_string(), "status".to_string()],
                    description: None,
                },
                PageTypeRule {
                    name: "architecture".to_string(),
                    naming_pattern: "<slug>".to_string(),
                    required_fields: vec!["title".to_string()],
                    description: None,
                },
            ],
            categories: vec![CategoryRule {
                name: "design".to_string(),
                allowed_page_types: vec!["decision".to_string(), "architecture".to_string()],
                description: None,
            }],
            cross_reference_rules: vec![CrossReferenceRule {
                from_type: "decision".to_string(),
                to_type: "architecture".to_string(),
                required: true,
            }],
        }
    }

    fn make_advisory_schema() -> KnowledgeSchema {
        KnowledgeSchema {
            enforcement_mode: EnforcementMode::Advisory,
            ..make_enforced_schema()
        }
    }

    // --- serde roundtrip tests ---

    #[test]
    fn knowledge_schema_serde_roundtrip() {
        let schema = make_enforced_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let parsed: KnowledgeSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, schema.name);
        assert_eq!(parsed.version, schema.version);
        assert_eq!(parsed.page_types.len(), schema.page_types.len());
        assert_eq!(parsed.categories.len(), schema.categories.len());
        assert_eq!(parsed.cross_reference_rules.len(), schema.cross_reference_rules.len());
    }

    #[test]
    fn enforcement_mode_serde_enforced() {
        let json = serde_json::to_string(&EnforcementMode::Enforced).unwrap();
        assert_eq!(json, "\"enforced\"");
        let parsed: EnforcementMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, EnforcementMode::Enforced);
    }

    #[test]
    fn enforcement_mode_serde_advisory() {
        let json = serde_json::to_string(&EnforcementMode::Advisory).unwrap();
        assert_eq!(json, "\"advisory\"");
        let parsed: EnforcementMode = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, EnforcementMode::Advisory);
    }

    // --- validate_page_name ---

    #[test]
    fn validate_page_name_valid_decision() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let violations = validator.validate_page_name("2024-03-15-my-decision", "decision", &schema);
        assert!(violations.is_empty(), "expected no violations, got: {violations:?}");
    }

    #[test]
    fn validate_page_name_bad_decision_name() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let violations = validator.validate_page_name("bad-name", "decision", &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "naming_pattern");
        assert_eq!(violations[0].severity, ViolationSeverity::Error);
    }

    #[test]
    fn validate_page_name_valid_slug() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let violations = validator.validate_page_name("my-architecture", "architecture", &schema);
        assert!(violations.is_empty(), "expected no violations, got: {violations:?}");
    }

    #[test]
    fn validate_page_name_unknown_type() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let violations = validator.validate_page_name("anything", "nonexistent", &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "unknown_page_type");
    }

    #[test]
    fn validate_page_name_advisory_mode_produces_warnings() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_advisory_schema();
        let violations = validator.validate_page_name("bad-name", "decision", &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].severity, ViolationSeverity::Warning);
    }

    // --- validate_required_fields ---

    #[test]
    fn validate_required_fields_all_present() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let mut fm = HashMap::new();
        fm.insert("title".to_string(), "My Decision".to_string());
        fm.insert("status".to_string(), "accepted".to_string());
        let violations = validator.validate_required_fields(&fm, "decision", &schema);
        assert!(violations.is_empty());
    }

    #[test]
    fn validate_required_fields_missing_field() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let mut fm = HashMap::new();
        fm.insert("title".to_string(), "My Decision".to_string());
        // "status" is missing
        let violations = validator.validate_required_fields(&fm, "decision", &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "required_field");
        assert!(violations[0].message.contains("status"));
        assert_eq!(violations[0].severity, ViolationSeverity::Error);
    }

    #[test]
    fn validate_required_fields_unknown_type() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let fm = HashMap::new();
        let violations = validator.validate_required_fields(&fm, "nonexistent", &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "unknown_page_type");
    }

    // --- validate_cross_references ---

    #[test]
    fn validate_cross_references_satisfied() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let refs = vec!["architecture".to_string()];
        let violations = validator.validate_cross_references("decision", &refs, &schema);
        assert!(violations.is_empty());
    }

    #[test]
    fn validate_cross_references_missing_required() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let refs: Vec<String> = vec![];
        let violations = validator.validate_cross_references("decision", &refs, &schema);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "cross_reference");
        assert_eq!(violations[0].severity, ViolationSeverity::Error);
    }

    #[test]
    fn validate_cross_references_non_required_not_flagged() {
        let validator = KnowledgeSchemaValidator::new();
        let mut schema = make_enforced_schema();
        // Make the rule optional
        schema.cross_reference_rules[0].required = false;
        let refs: Vec<String> = vec![];
        let violations = validator.validate_cross_references("decision", &refs, &schema);
        assert!(violations.is_empty());
    }

    // --- default_schema ---

    #[test]
    fn default_schema_is_valid_and_non_empty() {
        let schema = default_schema();
        assert!(!schema.name.is_empty());
        assert!(!schema.version.is_empty());
        assert!(!schema.page_types.is_empty());
        assert!(!schema.categories.is_empty());
        assert!(!schema.cross_reference_rules.is_empty());
    }

    #[test]
    fn default_schema_advisory_mode() {
        let schema = default_schema();
        assert_eq!(schema.enforcement_mode, EnforcementMode::Advisory);
    }

    #[test]
    fn default_schema_serde_roundtrip() {
        let schema = default_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let parsed: KnowledgeSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, schema.name);
        assert_eq!(parsed.page_types.len(), schema.page_types.len());
    }

    // --- advisory vs enforced mode ---

    #[test]
    fn enforced_mode_produces_errors() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_enforced_schema();
        let violations = validator.validate_page_name("wrong", "decision", &schema);
        assert!(violations.iter().all(|v| v.severity == ViolationSeverity::Error));
    }

    #[test]
    fn advisory_mode_produces_warnings() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_advisory_schema();
        let violations = validator.validate_page_name("wrong", "decision", &schema);
        assert!(violations.iter().all(|v| v.severity == ViolationSeverity::Warning));
    }

    // --- full validation scenario ---

    #[test]
    fn full_validation_advisory_multiple_violations() {
        let validator = KnowledgeSchemaValidator::new();
        let schema = make_advisory_schema();

        // Bad name
        let name_violations = validator.validate_page_name("bad-name", "decision", &schema);
        assert_eq!(name_violations.len(), 1);
        assert_eq!(name_violations[0].severity, ViolationSeverity::Warning);

        // Missing fields
        let fm: HashMap<String, String> = HashMap::new();
        let field_violations = validator.validate_required_fields(&fm, "decision", &schema);
        assert_eq!(field_violations.len(), 2); // title and status

        // Missing cross-reference
        let ref_violations = validator.validate_cross_references("decision", &[], &schema);
        assert_eq!(ref_violations.len(), 1);

        let all: Vec<_> = name_violations
            .into_iter()
            .chain(field_violations)
            .chain(ref_violations)
            .collect();
        assert_eq!(all.len(), 4);
        assert!(all.iter().all(|v| v.severity == ViolationSeverity::Warning));
    }
}
