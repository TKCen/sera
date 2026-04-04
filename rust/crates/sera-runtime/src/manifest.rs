//! Agent manifest loading and system prompt building.
//!
//! Loads AGENT.yaml manifests and assembles system prompts using priority-ordered sections.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Agent manifest — mirrors AGENT.yaml spec with both flat and spec-wrapped formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum RuntimeManifest {
    /// Spec-wrapped format (new): all config inside `spec` block
    SpecWrapped {
        #[serde(rename = "apiVersion")]
        api_version: String,
        kind: String,
        metadata: ManifestMetadata,
        spec: AgentSpec,
    },
    /// Flat format (legacy): identity and model at top-level
    Flat {
        #[serde(rename = "apiVersion")]
        api_version: String,
        kind: String,
        metadata: ManifestMetadata,
        identity: IdentityConfig,
        model: ModelConfig,
        #[serde(default)]
        tools: Vec<ToolConfig>,
        #[serde(default)]
        memory: MemoryConfig,
        #[serde(default)]
        intercom: IntercomConfig,
        #[serde(default)]
        subagents: Vec<SubagentConfig>,
        #[serde(default)]
        contextFiles: Vec<String>,
        #[serde(default)]
        bootContext: BootContext,
        #[serde(default)]
        outputFormat: OutputFormat,
    },
}

/// Manifest metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMetadata {
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// Agent spec (spec-wrapped format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub identity: IdentityConfig,
    pub model: ModelConfig,
    #[serde(default)]
    pub tools: Vec<ToolConfig>,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub intercom: IntercomConfig,
    #[serde(default)]
    pub subagents: Vec<SubagentConfig>,
    #[serde(default)]
    pub contextFiles: Vec<String>,
    #[serde(default)]
    pub bootContext: BootContext,
    #[serde(default)]
    pub outputFormat: OutputFormat,
}

/// Agent identity configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub role: String,
    #[serde(default)]
    pub bio: String,
    #[serde(default)]
    pub instructions: String,
}

/// LLM model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// Tool configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
}

/// Memory configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub maxBlocks: usize,
}

/// Intercom configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntercomConfig {
    #[serde(default)]
    pub enabled: bool,
}

/// Subagent reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    pub name: String,
}

/// Boot context — initial context provided at startup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BootContext {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

/// Output format specification.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputFormat {
    #[serde(default)]
    pub contentType: String,
}

impl RuntimeManifest {
    /// Get identity config from either format.
    pub fn identity(&self) -> &IdentityConfig {
        match self {
            RuntimeManifest::SpecWrapped { spec, .. } => &spec.identity,
            RuntimeManifest::Flat { identity, .. } => identity,
        }
    }

    /// Get model config from either format.
    pub fn model(&self) -> &ModelConfig {
        match self {
            RuntimeManifest::SpecWrapped { spec, .. } => &spec.model,
            RuntimeManifest::Flat { model, .. } => model,
        }
    }

    /// Get tools list from either format.
    pub fn tools(&self) -> &[ToolConfig] {
        match self {
            RuntimeManifest::SpecWrapped { spec, .. } => &spec.tools,
            RuntimeManifest::Flat { tools, .. } => tools,
        }
    }

    /// Get memory config from either format.
    pub fn memory(&self) -> &MemoryConfig {
        match self {
            RuntimeManifest::SpecWrapped { spec, .. } => &spec.memory,
            RuntimeManifest::Flat { memory, .. } => memory,
        }
    }

    /// Get boot context from either format.
    pub fn boot_context(&self) -> &BootContext {
        match self {
            RuntimeManifest::SpecWrapped { spec, .. } => &spec.bootContext,
            RuntimeManifest::Flat { bootContext, .. } => bootContext,
        }
    }
}

/// Load manifest from AGENT.yaml file.
pub fn load_manifest<P: AsRef<Path>>(path: P) -> anyhow::Result<RuntimeManifest> {
    let content = std::fs::read_to_string(path)?;
    let manifest = serde_yaml::from_str::<RuntimeManifest>(&content)?;
    Ok(manifest)
}

/// A section of the system prompt with priority ordering.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PromptSection {
    pub id: String,
    pub priority: u32,
    pub content: String,
    pub required: bool,
}

/// Builds system prompt from manifest with priority-based composition and token budgets.
#[allow(dead_code)]
pub struct SystemPromptBuilder {
    sections: Vec<PromptSection>,
    token_budget: usize,
}

impl SystemPromptBuilder {
    /// Create a new SystemPromptBuilder with token budget.
    pub fn new(token_budget: usize) -> Self {
        Self {
            sections: Vec::new(),
            token_budget,
        }
    }

    /// Add an identity section (agent role, bio, principles).
    pub fn add_identity(mut self, role: &str, bio: &str) -> Self {
        let content = if bio.is_empty() {
            format!("You are a {}.", role)
        } else {
            format!("You are a {}. {}", role, bio)
        };
        self.sections.push(PromptSection {
            id: "identity".to_string(),
            priority: 10, // High priority
            content,
            required: true,
        });
        self
    }

    /// Add instructions section.
    pub fn add_instructions(mut self, instructions: &str) -> Self {
        if !instructions.is_empty() {
            self.sections.push(PromptSection {
                id: "instructions".to_string(),
                priority: 20,
                content: format!("Instructions:\n{}", instructions),
                required: true,
            });
        }
        self
    }

    /// Add available tools section.
    pub fn add_available_tools(mut self, tools: &str) -> Self {
        if !tools.is_empty() {
            self.sections.push(PromptSection {
                id: "available_tools".to_string(),
                priority: 30,
                content: format!("Available tools:\n{}", tools),
                required: false,
            });
        }
        self
    }

    /// Add tool usage guidelines section.
    pub fn add_tool_guidelines(mut self, guidelines: &str) -> Self {
        if !guidelines.is_empty() {
            self.sections.push(PromptSection {
                id: "tool_guidelines".to_string(),
                priority: 40,
                content: guidelines.to_string(),
                required: false,
            });
        }
        self
    }

    /// Add memory instructions section.
    pub fn add_memory_instructions(mut self, instructions: &str) -> Self {
        if !instructions.is_empty() {
            self.sections.push(PromptSection {
                id: "memory".to_string(),
                priority: 50,
                content: instructions.to_string(),
                required: false,
            });
        }
        self
    }

    /// Add time/timezone context section.
    pub fn add_time_context(mut self, context: &str) -> Self {
        if !context.is_empty() {
            self.sections.push(PromptSection {
                id: "time_context".to_string(),
                priority: 60,
                content: context.to_string(),
                required: false,
            });
        }
        self
    }

    /// Add circle/shared memory context section.
    pub fn add_circle_context(mut self, context: &str) -> Self {
        if !context.is_empty() {
            self.sections.push(PromptSection {
                id: "circle_context".to_string(),
                priority: 70,
                content: context.to_string(),
                required: false,
            });
        }
        self
    }

    /// Add delegation/subagent context section.
    pub fn add_delegation_context(mut self, context: &str) -> Self {
        if !context.is_empty() {
            self.sections.push(PromptSection {
                id: "delegation".to_string(),
                priority: 80,
                content: context.to_string(),
                required: false,
            });
        }
        self
    }

    /// Add custom section with explicit priority.
    pub fn add_section(mut self, id: &str, priority: u32, content: &str, required: bool) -> Self {
        self.sections.push(PromptSection {
            id: id.to_string(),
            priority,
            content: content.to_string(),
            required,
        });
        self
    }

    /// Build the system prompt respecting token budget and priority ordering.
    ///
    /// Strategy:
    /// 1. Sort sections by priority (lower number = higher priority)
    /// 2. Include all required sections first
    /// 3. Add optional sections in priority order until budget exhausted
    /// 4. Return assembled prompt string
    pub fn build(&self) -> String {
        // Sort by priority (ascending = higher priority first)
        let mut sorted = self.sections.clone();
        sorted.sort_by_key(|s| s.priority);

        let mut result = String::new();
        let mut token_count = 0;

        // 1. Add required sections first
        for section in &sorted {
            if section.required {
                let tokens = estimate_tokens(&section.content);
                if token_count + tokens <= self.token_budget {
                    if !result.is_empty() {
                        result.push_str("\n\n");
                    }
                    result.push_str(&section.content);
                    token_count += tokens;
                }
            }
        }

        // 2. Add optional sections in priority order
        for section in &sorted {
            if !section.required {
                let tokens = estimate_tokens(&section.content);
                if token_count + tokens <= self.token_budget {
                    if !result.is_empty() {
                        result.push_str("\n\n");
                    }
                    result.push_str(&section.content);
                    token_count += tokens;
                }
            }
        }

        result
    }
}

/// Rough token estimation: 4 characters ≈ 1 token (cl100k_base encoding).
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_identity_spec_wrapped() {
        let manifest = RuntimeManifest::SpecWrapped {
            api_version: "v1".to_string(),
            kind: "Agent".to_string(),
            metadata: ManifestMetadata {
                name: "test-agent".to_string(),
                namespace: "default".to_string(),
                labels: BTreeMap::new(),
            },
            spec: AgentSpec {
                identity: IdentityConfig {
                    role: "test".to_string(),
                    bio: "test bio".to_string(),
                    instructions: "test instructions".to_string(),
                },
                model: ModelConfig {
                    name: "gpt-4".to_string(),
                    provider: "openai".to_string(),
                    reasoning: false,
                    temperature: None,
                },
                tools: vec![],
                memory: MemoryConfig::default(),
                intercom: IntercomConfig::default(),
                subagents: vec![],
                contextFiles: vec![],
                bootContext: BootContext::default(),
                outputFormat: OutputFormat::default(),
            },
        };

        assert_eq!(manifest.identity().role, "test");
        assert_eq!(manifest.model().name, "gpt-4");
    }

    #[test]
    fn test_system_prompt_builder() {
        let builder = SystemPromptBuilder::new(10_000)
            .add_identity("researcher", "A helpful research agent")
            .add_instructions("Use tools to gather information")
            .add_available_tools("search, read, write");

        let prompt = builder.build();
        assert!(prompt.contains("researcher"));
        assert!(prompt.contains("Instructions:"));
        assert!(prompt.contains("Available tools:"));
    }

    #[test]
    fn test_system_prompt_respects_budget() {
        let builder = SystemPromptBuilder::new(20) // Very small budget
            .add_identity("x", "")
            .add_instructions("a very long instruction that will definitely exceed our token budget");

        let prompt = builder.build();
        // Only identity (required) should fit
        assert!(prompt.contains("x"));
        assert!(!prompt.contains("very long instruction"));
    }

    #[test]
    fn test_token_estimation() {
        assert_eq!(estimate_tokens("hello"), 2); // 5 chars / 4
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
    }
}
