//! Enterprise secrets provider scaffolds.
//!
//! These structs are forward-looking placeholders for future provider implementations.
//! They intentionally do NOT implement [`crate::SecretsProvider`] yet — they serve as
//! documentation anchors and will be fleshed out when the corresponding SDK integrations
//! land.
//!
//! The tests at the bottom of this file pin that contract: they verify that the stubs
//! can be constructed but cannot be used as `dyn SecretsProvider`. This prevents an
//! accidental "silent success" impl where a future refactor adds a no-op trait
//! implementation that would return empty secrets instead of surfacing an error.

/// HashiCorp Vault secrets provider (not yet implemented).
///
/// Will use the [`vaultrs`](https://crates.io/crates/vaultrs) crate to authenticate and
/// fetch secrets from a Vault KV store. Supports AppRole, Kubernetes, and token auth methods.
///
/// Planned dependency: `vaultrs = "0.7"`
///
/// # Doc-test: stub must not implement `SecretsProvider`
///
/// ```compile_fail
/// use sera_secrets::SecretsProvider;
/// use sera_secrets::enterprise::VaultSecretsProvider;
/// fn must_be_provider<T: SecretsProvider>() {}
/// must_be_provider::<VaultSecretsProvider>();
/// ```
pub struct VaultSecretsProvider;

/// AWS Secrets Manager provider (not yet implemented).
///
/// Will use the [`aws-sdk-secretsmanager`](https://crates.io/crates/aws-sdk-secretsmanager)
/// crate (AWS SDK for Rust) to retrieve secrets from AWS Secrets Manager. Supports IAM role
/// and explicit credential authentication.
///
/// Planned dependency: `aws-sdk-secretsmanager = "1"`
///
/// # Doc-test: stub must not implement `SecretsProvider`
///
/// ```compile_fail
/// use sera_secrets::SecretsProvider;
/// use sera_secrets::enterprise::AwsSecretsProvider;
/// fn must_be_provider<T: SecretsProvider>() {}
/// must_be_provider::<AwsSecretsProvider>();
/// ```
pub struct AwsSecretsProvider;

/// Azure Key Vault secrets provider (not yet implemented).
///
/// Will use the [`azure_security_keyvault_secrets`](https://crates.io/crates/azure_security_keyvault_secrets)
/// crate to fetch secrets from an Azure Key Vault instance. Supports managed identity and
/// service principal authentication.
///
/// Planned dependency: `azure_security_keyvault_secrets = "0.20"`
///
/// # Doc-test: stub must not implement `SecretsProvider`
///
/// ```compile_fail
/// use sera_secrets::SecretsProvider;
/// use sera_secrets::enterprise::AzureSecretsProvider;
/// fn must_be_provider<T: SecretsProvider>() {}
/// must_be_provider::<AzureSecretsProvider>();
/// ```
pub struct AzureSecretsProvider;

#[cfg(test)]
mod tests {
    use super::*;

    // Doc-tests above (`compile_fail`) verify that the stubs do NOT implement
    // `SecretsProvider`. These runtime tests lock in the remaining half of
    // the contract: the stubs remain constructible as documentation anchors,
    // and the type-ids are distinct (guards against accidental merging).
    #[test]
    fn stubs_are_constructible() {
        let _ = VaultSecretsProvider;
        let _ = AwsSecretsProvider;
        let _ = AzureSecretsProvider;
    }

    #[test]
    fn stubs_are_distinct_types() {
        use std::any::TypeId;
        assert_ne!(
            TypeId::of::<VaultSecretsProvider>(),
            TypeId::of::<AwsSecretsProvider>()
        );
        assert_ne!(
            TypeId::of::<AwsSecretsProvider>(),
            TypeId::of::<AzureSecretsProvider>()
        );
        assert_ne!(
            TypeId::of::<VaultSecretsProvider>(),
            TypeId::of::<AzureSecretsProvider>()
        );
    }
}
