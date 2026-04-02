import os
import re

modules = {
    'agents': ['Orchestrator', 'AgentRegistry', 'AgentManifest'],
    'llm': ['LlmRouter', 'ProviderRegistry', 'ChatMessage'],
    'sandbox': ['SandboxManager', 'SandboxInfo', 'SpawnRequest'],
    'memory': ['ScopedMemoryBlockStore', 'KnowledgeBlock'],
    'skills': ['SkillRegistry', 'SkillDefinition'],
    'mcp': ['MCPRegistry'],
    'auth': ['AuthService', 'IdentityService'],
    'sessions': ['SessionStore'],
    'audit': ['AuditService']
}

module_files = {
    'agents': ['Orchestrator.js', 'registry.service.js', 'manifest/types.js', 'BaseAgent.js', 'AgentFactory.js', 'WorkerAgent.js', 'SubagentRunner.js', 'HeartbeatService.js', 'CleanupService.js', 'types.js', 'identity/IdentityService.js'],
    'llm': ['LlmRouter.js', 'ProviderRegistry.js', 'ContextAssembler.js', 'ContextCompactionService.js', 'DynamicProviderManager.js', 'CircuitBreakerService.js', 'ProviderHealthService.js'],
    'sandbox': ['SandboxManager.js', 'types.js', 'WorktreeManager.js', 'PermissionRequestService.js', 'TierPolicy.js', 'ToolRunner.js', 'EgressAclManager.js', 'ContainerSecurityMapper.js'],
    'memory': ['blocks/ScopedMemoryBlockStore.js', 'blocks/scoped-types.js', 'manager.js', 'KnowledgeGitService.js', 'MemoryCompactionService.js'],
    'skills': ['SkillRegistry.js', 'types.js', 'SkillInjector.js', 'SkillLibrary.js', 'adapters/SkillRegistryService.js'],
    'mcp': ['registry.js', 'MCPServerManager.js'],
    'auth': ['auth-service.js', 'IdentityService.js', 'authMiddleware.js', 'web-session-store.js', 'interfaces.js', 'api-key-service.js'],
    'sessions': ['SessionStore.js'],
    'audit': ['AuditService.js']
}

def update_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    original_content = content

    # Extract relative path to core/src
    rel_to_src = os.path.relpath('core/src', os.path.dirname(filepath))
    if rel_to_src == '.':
        rel_to_src = '.'
    else:
        rel_to_src = './' + rel_to_src if not rel_to_src.startswith('.') else rel_to_src

    for module, files in module_files.items():
        for filename in files:
            # Pattern to match imports from internal files
            # Matches: import ... from '../module/file.js' or './file.js' (if in same module)

            # Cross-module imports
            pattern = rf"from '(\.\.?/)+(?:[^/]+/)*{module}/{filename}'"

            # Determine if we should replace based on the target module
            # If the file being updated is NOT in the target module, we MUST use the barrel.
            file_dir = os.path.dirname(filepath)
            is_in_module = f"core/src/{module}" in file_dir or filepath.endswith(f"core/src/{module}.ts")

            if not is_in_module:
                 # It's a cross-module import, should use barrel
                 # We need to find where the module's index.js is relative to this file

                 # Simplified approach: if it was importing from module/file.js, change to module/index.js
                 regex = rf"from '([^']+)/{module}/{filename}'"
                 content = re.sub(regex, rf"from '\1/{module}/index.js'", content)

                 # Also handle cases where it might be importing from a subfolder of the module
                 # (This is already covered by the regex above if {module} is part of the path)
            else:
                 # Internal module import. ADR-005 says "defines a clear public API boundary".
                 # Usually internal files can still import from each other directly to avoid circular deps.
                 # "No module internals are imported directly from outside the module"
                 pass

    if content != original_content:
        with open(filepath, 'w') as f:
            f.write(content)
        return True
    return False

# Walk through core/src
count = 0
for root, dirs, files in os.walk('core/src'):
    for file in files:
        if file.endswith('.ts') and not file.endswith('.test.ts'):
            if update_file(os.path.join(root, file)):
                count += 1

print(f"Updated {count} files.")
