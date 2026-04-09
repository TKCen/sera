use std::collections::HashMap;

use crate::{
    error::WorkflowError,
    types::{WorkflowDef, WorkflowTrigger},
};

/// In-memory registry of workflow definitions.
#[derive(Debug, Default)]
pub struct WorkflowRegistry {
    workflows: HashMap<String, WorkflowDef>,
}

impl WorkflowRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a workflow definition.
    ///
    /// Returns [`WorkflowError::DuplicateWorkflow`] if a workflow with the
    /// same name is already registered.
    pub fn register(&mut self, def: WorkflowDef) -> Result<(), WorkflowError> {
        if self.workflows.contains_key(&def.name) {
            return Err(WorkflowError::DuplicateWorkflow { name: def.name });
        }
        self.workflows.insert(def.name.clone(), def);
        Ok(())
    }

    /// Remove a workflow by name.  Returns `true` if it existed.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.workflows.remove(name).is_some()
    }

    /// Look up a workflow by name.
    pub fn get(&self, name: &str) -> Option<&WorkflowDef> {
        self.workflows.get(name)
    }

    /// Return all registered workflows in unspecified order.
    pub fn list(&self) -> Vec<&WorkflowDef> {
        self.workflows.values().collect()
    }

    /// Return all enabled workflows.
    pub fn list_enabled(&self) -> Vec<&WorkflowDef> {
        self.workflows.values().filter(|w| w.enabled).collect()
    }

    /// Return all workflows whose trigger is [`WorkflowTrigger::Cron`].
    pub fn list_cron(&self) -> Vec<&WorkflowDef> {
        self.workflows
            .values()
            .filter(|w| matches!(w.trigger, WorkflowTrigger::Cron(_)))
            .collect()
    }

    /// Enable a workflow by name.  Returns `true` if the workflow was found.
    pub fn enable(&mut self, name: &str) -> bool {
        if let Some(w) = self.workflows.get_mut(name) {
            w.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a workflow by name.  Returns `true` if the workflow was found.
    pub fn disable(&mut self, name: &str) -> bool {
        if let Some(w) = self.workflows.get_mut(name) {
            w.enabled = false;
            true
        } else {
            false
        }
    }
}
