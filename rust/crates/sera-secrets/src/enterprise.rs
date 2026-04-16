//! Enterprise secrets provider scaffolds.
//!
//! These structs are forward-looking placeholders for future provider implementations.
//! They intentionally do NOT implement `SecretsProvider` yet — they serve as documentation
//! anchors and will be fleshed out when the corresponding SDK integrations land.

/// HashiCorp Vault secrets provider (not yet implemented).
///
/// Will use the [`vaultrs`](https://crates.io/crates/vaultrs) crate to authenticate and
/// fetch secrets from a Vault KV store. Supports AppRole, Kubernetes, and token auth methods.
///
/// Planned dependency: `vaultrs = "0.7"`
pub struct VaultSecretsProvider;

/// AWS Secrets Manager provider (not yet implemented).
///
/// Will use the [`aws-sdk-secretsmanager`](https://crates.io/crates/aws-sdk-secretsmanager)
/// crate (AWS SDK for Rust) to retrieve secrets from AWS Secrets Manager. Supports IAM role
/// and explicit credential authentication.
///
/// Planned dependency: `aws-sdk-secretsmanager = "1"`
pub struct AwsSecretsProvider;

/// Azure Key Vault secrets provider (not yet implemented).
///
/// Will use the [`azure_security_keyvault_secrets`](https://crates.io/crates/azure_security_keyvault_secrets)
/// crate to fetch secrets from an Azure Key Vault instance. Supports managed identity and
/// service principal authentication.
///
/// Planned dependency: `azure_security_keyvault_secrets = "0.20"`
pub struct AzureSecretsProvider;
