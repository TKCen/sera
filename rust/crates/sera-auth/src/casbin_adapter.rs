//! Casbin-backed authorization adapter for SERA.
//!
//! Wraps the casbin `Enforcer` with a thin interface that accepts model and
//! policy as in-memory strings. This makes it straightforward to embed RBAC
//! policies that live in the SERA config store rather than on disk.

use casbin::{CoreApi, DefaultModel, Enforcer, MgmtApi, StringAdapter};
use thiserror::Error;

/// Errors returned by [`CasbinAuthzAdapter`].
#[derive(Debug, Error)]
pub enum CasbinError {
    #[error("casbin error: {0}")]
    Casbin(String),
}

impl From<casbin::Error> for CasbinError {
    fn from(err: casbin::Error) -> Self {
        CasbinError::Casbin(err.to_string())
    }
}

/// Thin wrapper around a casbin [`Enforcer`] that accepts model and policy
/// as in-memory strings.
///
/// # Example
///
/// ```rust,ignore
/// let model = r#"
/// [request_definition]
/// r = sub, obj, act
///
/// [policy_definition]
/// p = sub, obj, act
///
/// [policy_effect]
/// e = some(where (p.eft == allow))
///
/// [matchers]
/// m = r.sub == p.sub && r.obj == p.obj && r.act == p.act
/// "#;
///
/// let policy = "p, alice, data1, read\np, bob, data2, write\n";
/// let adapter = CasbinAuthzAdapter::from_strings(model, policy).await?;
/// assert!(adapter.enforce("alice", "data1", "read").await?);
/// ```
pub struct CasbinAuthzAdapter {
    enforcer: Enforcer,
}

impl CasbinAuthzAdapter {
    /// Construct a `CasbinAuthzAdapter` from a model definition string and a
    /// policy CSV string (same format as casbin `.conf` and `.csv` files).
    pub async fn from_strings(
        model_text: &str,
        policy_text: &str,
    ) -> Result<Self, CasbinError> {
        let model = DefaultModel::from_str(model_text)
            .await
            .map_err(CasbinError::from)?;
        let adapter = StringAdapter::new(policy_text);
        let enforcer: Enforcer = Enforcer::new(model, adapter)
            .await
            .map_err(CasbinError::from)?;
        Ok(Self { enforcer })
    }

    /// Evaluate whether `subject` may perform `action` on `object`.
    pub async fn enforce(
        &self,
        subject: &str,
        object: &str,
        action: &str,
    ) -> Result<bool, CasbinError> {
        use casbin::CoreApi;
        self.enforcer
            .enforce((subject, object, action))
            .map_err(CasbinError::from)
    }

    /// Add a policy rule at runtime: `(subject, object, action)`.
    pub async fn add_policy(
        &mut self,
        subject: &str,
        object: &str,
        action: &str,
    ) -> Result<bool, CasbinError> {
        self.enforcer
            .add_policy(vec![
                subject.to_string(),
                object.to_string(),
                action.to_string(),
            ])
            .await
            .map_err(CasbinError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal RBAC model (sub/obj/act equality matching).
    const BASIC_MODEL: &str = r#"[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && r.obj == p.obj && r.act == p.act
"#;

    #[tokio::test]
    async fn basic_allow_deny() {
        let policy = "p, alice, data1, read\np, bob, data2, write\n";
        let adapter = CasbinAuthzAdapter::from_strings(BASIC_MODEL, policy)
            .await
            .expect("adapter init");

        assert!(adapter.enforce("alice", "data1", "read").await.unwrap());
        assert!(!adapter.enforce("alice", "data2", "write").await.unwrap());
        assert!(adapter.enforce("bob", "data2", "write").await.unwrap());
        assert!(!adapter.enforce("bob", "data1", "read").await.unwrap());
    }
}
