use serde::{Deserialize, Serialize};
use std::fmt;

/// Three-part scope: `resource[:identifier]:action`.
///
/// Matching rules:
/// - Exact: `agents:triage:run` grants `agents:triage:run`
/// - Two-part grants all identifiers: `agents:run` grants `agents:triage:run`
/// - Wildcard: `agents:*:run` grants `agents:triage:run`
/// - `admin:*` grants everything
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    pub resource: String,
    pub identifier: Option<String>,
    pub action: String,
}

impl Scope {
    /// Parse a scope string like "agents:run", "agents:triage:run", or "admin:*".
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            2 => Ok(Scope {
                resource: parts[0].to_string(),
                identifier: None,
                action: parts[1].to_string(),
            }),
            3 => Ok(Scope {
                resource: parts[0].to_string(),
                identifier: Some(parts[1].to_string()),
                action: parts[2].to_string(),
            }),
            _ => Err(format!("invalid scope format: {s}")),
        }
    }

    /// Check if this scope grants the requested scope.
    pub fn grants(&self, requested: &Scope) -> bool {
        // admin:* grants everything
        if self.resource == "admin" && self.action == "*" {
            return true;
        }

        // Resource must match
        if self.resource != requested.resource {
            return false;
        }

        // Action must match (or grant is wildcard)
        if self.action != "*" && self.action != requested.action {
            return false;
        }

        // Identifier matching:
        match (&self.identifier, &requested.identifier) {
            // Two-part grant (no identifier) → grants all identifiers
            (None, _) => true,
            // Explicit wildcard → grants all identifiers
            (Some(id), _) if id == "*" => true,
            // Exact match
            (Some(grant_id), Some(req_id)) => grant_id == req_id,
            // Three-part grant, two-part request → grants (the resource:action is already matched)
            (Some(_), None) => false,
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.identifier {
            Some(id) => write!(f, "{}:{}:{}", self.resource, id, self.action),
            None => write!(f, "{}:{}", self.resource, self.action),
        }
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Scope::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Convenience: check if any scope in a list grants the requested scope.
pub fn has_scope(scopes: &[Scope], requested: &Scope) -> bool {
    scopes.iter().any(|s| s.grants(requested))
}

/// Parse a list of scope strings into Scope objects.
pub fn parse_scopes(strings: &[String]) -> Result<Vec<Scope>, String> {
    strings.iter().map(|s| Scope::parse(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(s: &str) -> Scope {
        Scope::parse(s).unwrap()
    }

    #[test]
    fn exact_match() {
        assert!(scope("agents:triage:run").grants(&scope("agents:triage:run")));
    }

    #[test]
    fn two_part_grants_three_part() {
        // agents:run grants agents:triage:run
        assert!(scope("agents:run").grants(&scope("agents:triage:run")));
    }

    #[test]
    fn two_part_grants_two_part() {
        assert!(scope("agents:run").grants(&scope("agents:run")));
    }

    #[test]
    fn explicit_wildcard() {
        assert!(scope("agents:*:run").grants(&scope("agents:triage:run")));
        assert!(scope("inference:*:call").grants(&scope("inference:sonnet-fast:call")));
    }

    #[test]
    fn admin_star_grants_everything() {
        assert!(scope("admin:*").grants(&scope("agents:triage:run")));
        assert!(scope("admin:*").grants(&scope("inference:call")));
        assert!(scope("admin:*").grants(&scope("platform:admin")));
        assert!(scope("admin:*").grants(&scope("knowledge:write")));
    }

    #[test]
    fn wrong_resource_denied() {
        assert!(!scope("agents:run").grants(&scope("inference:call")));
    }

    #[test]
    fn wrong_action_denied() {
        assert!(!scope("agents:run").grants(&scope("agents:manage")));
    }

    #[test]
    fn three_part_does_not_grant_different_identifier() {
        assert!(!scope("agents:triage:run").grants(&scope("agents:devops:run")));
    }

    #[test]
    fn three_part_does_not_grant_two_part() {
        // agents:triage:run does NOT grant agents:run (all agents)
        assert!(!scope("agents:triage:run").grants(&scope("agents:run")));
    }

    #[test]
    fn inference_scopes() {
        assert!(scope("inference:sonnet-fast:call").grants(&scope("inference:sonnet-fast:call")));
        assert!(!scope("inference:sonnet-fast:call").grants(&scope("inference:gpt4o:call")));
        assert!(scope("inference:call").grants(&scope("inference:sonnet-fast:call")));
        assert!(scope("inference:call").grants(&scope("inference:gpt4o:call")));
    }

    #[test]
    fn scope_display_roundtrip() {
        let s = scope("agents:triage:run");
        assert_eq!(s.to_string(), "agents:triage:run");

        let s2 = scope("agents:run");
        assert_eq!(s2.to_string(), "agents:run");

        let s3 = scope("admin:*");
        assert_eq!(s3.to_string(), "admin:*");
    }

    #[test]
    fn has_scope_check() {
        let scopes = vec![
            scope("inference:sonnet-fast:call"),
            scope("agents:triage:run"),
        ];
        assert!(has_scope(&scopes, &scope("agents:triage:run")));
        assert!(has_scope(&scopes, &scope("inference:sonnet-fast:call")));
        assert!(!has_scope(&scopes, &scope("agents:devops:run")));
        assert!(!has_scope(&scopes, &scope("inference:gpt4o:call")));
    }

    #[test]
    fn parse_invalid() {
        assert!(Scope::parse("").is_err());
        assert!(Scope::parse("single").is_err());
        assert!(Scope::parse("a:b:c:d").is_err());
    }
}
