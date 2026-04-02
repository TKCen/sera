import os
import re

modules = ['agents', 'llm', 'sandbox', 'memory', 'skills', 'mcp', 'auth', 'sessions', 'audit']

module_files = {
    'agents': ['Orchestrator.js', 'registry.service.js', 'manifest/AgentManifestLoader.js', 'BaseAgent.js', 'AgentFactory.js', 'WorkerAgent.js', 'SubagentRunner.js', 'HeartbeatService.js', 'CleanupService.js', 'types.js', 'identity/IdentityService.js', 'importer.service.js', 'bootstrap.service.js'],
    'llm': ['LlmRouter.js', 'ProviderRegistry.js', 'ContextAssembler.js', 'ContextCompactionService.js', 'DynamicProviderManager.js', 'CircuitBreakerService.js', 'ProviderHealthService.js'],
    'sandbox': ['SandboxManager.js', 'types.js', 'WorktreeManager.js', 'PermissionRequestService.js', 'TierPolicy.js', 'ToolRunner.js', 'EgressAclManager.js', 'ContainerSecurityMapper.js'],
    'memory': ['blocks/ScopedMemoryBlockStore.js', 'blocks/scoped-types.js', 'manager.js', 'KnowledgeGitService.js', 'MemoryCompactionService.js', 'Reflector.js'],
    'skills': ['SkillRegistry.js', 'types.js', 'SkillInjector.js', 'SkillLibrary.js', 'adapters/SkillRegistryService.js'],
    'mcp': ['registry.js', 'MCPServerManager.js', 'SeraMCPServer.js'],
    'auth': ['auth-service.js', 'IdentityService.js', 'authMiddleware.js', 'web-session-store.js', 'interfaces.js', 'api-key-service.js', 'api-key-provider.js', 'oidc-provider.js'],
    'sessions': ['SessionStore.js'],
    'audit': ['AuditService.js']
}

def fix_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    original_content = content

    file_dir = os.path.dirname(filepath)

    for module in modules:
        # Check if the file is in this module
        if f"core/src/{module}" in filepath:
            # If it's importing from its own index.js, change it back to direct files
            # This is tricky because we don't know which file it actually wants.
            # But wait, my previous script changed module/file.js to module/index.js.
            # For files WITHIN the module, it should have been module/file.js.

            # Find imports like from './index.js' or from '../index.js' (if in subfolder)
            # and they ARE in the module's directory.

            # Since I know which files are in which module, I can look for what's MISSING.
            # Actually, the error was "WorkerAgent is not a constructor" which happens with circular deps in index.ts.

            # Let's just revert all internal index.js imports to direct imports if we can.
            # I'll use a mapping of common classes to files.
            pass

    # A better approach: revert all `from './index.js'` and `from '../index.js'`
    # to direct imports if the file is within the same module.

    # Let's try to find which module the file belongs to
    match = re.search(r'core/src/([^/]+)', filepath)
    if match:
        current_module = match.group(1)
        if current_module in modules:
            # Revert imports from index.js within the same module

            # We need to know which symbol comes from which file.
            symbol_to_file = {
                'Orchestrator': 'Orchestrator.js',
                'AgentRegistry': 'registry.service.js',
                'AgentManifestLoader': 'manifest/AgentManifestLoader.js',
                'BaseAgent': 'BaseAgent.js',
                'AgentFactory': 'AgentFactory.js',
                'WorkerAgent': 'WorkerAgent.js',
                'HeartbeatService': 'HeartbeatService.js',
                'CleanupService': 'CleanupService.js',
                'ResourceImporter': 'importer.service.js',
                'LlmRouter': 'LlmRouter.js',
                'ProviderRegistry': 'ProviderRegistry.js',
                'ContextAssembler': 'ContextAssembler.js',
                'ContextCompactionService': 'ContextCompactionService.js',
                'CircuitBreakerService': 'CircuitBreakerService.js',
                'DynamicProviderManager': 'DynamicProviderManager.js',
                'SandboxManager': 'SandboxManager.js',
                'MemoryManager': 'manager.js',
                'SkillRegistry': 'SkillRegistry.js',
                'SkillLibrary': 'SkillLibrary.js',
                'SkillInjector': 'SkillInjector.js',
                'MCPRegistry': 'registry.js',
                'MCPServerManager': 'MCPServerManager.js',
                'AuthService': 'auth-service.js',
                'IdentityService': 'IdentityService.js',
                'SessionStore': 'SessionStore.js',
                'AuditService': 'AuditService.js',
                'KNOWLEDGE_BLOCK_TYPES': 'blocks/scoped-types.js'
            }

            for symbol, filename in symbol_to_file.items():
                # import { ... Symbol ... } from './index.js'
                # or from '../index.js'

                # Check if this symbol belongs to the current module
                is_symbol_in_module = False
                for mod, files in module_files.items():
                    if filename in files and mod == current_module:
                        is_symbol_in_module = True
                        break

                if is_symbol_in_module:
                    # Determine relative path to the file
                    # This is simplified: assuming everything is in the root of the module or one level deep

                    # Pattern for ./index.js
                    pattern = rf"\{{([^}}]*\b{symbol}\b[^}}]*)\}} from '\./index.js'"
                    # We need to know if the file is in the root or subfolder
                    # and where the target file is.

                    # Instead of complex logic, let's just use the known locations.
                    # If it's in the same module, we can use the direct relative path.

                    # For now, let's just revert the ones I KNOW are causing issues.
                    pass

    # Actually, the easiest way to fix circularity in index.ts is to NOT import from index.ts
    # within the same module.

    # Let's just do a blanket revert for any file in core/src/MODULE/* importing from ./index.js or ../index.js
    # and manually fix the few that need it.

    match = re.search(r'core/src/(agents|llm|sandbox|memory|skills|mcp|auth|sessions|audit)', filepath)
    if match:
        module = match.group(1)

        # If it's core/src/agents/AgentFactory.ts and it imports from ./index.js
        # we want to change it back to the specific files.

        if module == 'agents':
            content = content.replace("from './index.js'", "from './NOT_INDEX'") # temporary
            content = re.sub(r'\{([^}]*\bOrchestrator\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./Orchestrator.js"', content)
            content = re.sub(r'\{([^}]*\bAgentRegistry\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./registry.service.js"', content)
            content = re.sub(r'\{([^}]*\bAgentManifestLoader\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./manifest/AgentManifestLoader.js"', content)
            content = re.sub(r'\{([^}]*\bBaseAgent\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./BaseAgent.js"', content)
            content = re.sub(r'\{([^}]*\bAgentFactory\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./AgentFactory.js"', content)
            content = re.sub(r'\{([^}]*\bWorkerAgent\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./WorkerAgent.js"', content)
            content = re.sub(r'\{([^}]*\bHeartbeatService\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./HeartbeatService.js"', content)
            content = re.sub(r'\{([^}]*\bCleanupService\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./CleanupService.js"', content)
            content = re.sub(r'\{([^}]*\bResourceImporter\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./importer.service.js"', content)
            content = content.replace("from './NOT_INDEX'", "from './index.js'") # revert remaining

        if module == 'llm':
            content = content.replace("from './index.js'", "from './NOT_INDEX'")
            content = re.sub(r'\{([^}]*\bLlmRouter\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./LlmRouter.js"', content)
            content = re.sub(r'\{([^}]*\bProviderRegistry\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./ProviderRegistry.js"', content)
            content = re.sub(r'\{([^}]*\bContextAssembler\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./ContextAssembler.js"', content)
            content = re.sub(r'\{([^}]*\bContextCompactionService\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./ContextCompactionService.js"', content)
            content = content.replace("from './NOT_INDEX'", "from './index.js'")

        if module == 'memory':
            content = content.replace("from './index.js'", "from './NOT_INDEX'")
            content = re.sub(r'\{([^}]*\bMemoryManager\b[^}]*)\} from \'./NOT_INDEX\'', r'{\1} from "./manager.js"', content)
            content = content.replace("from './NOT_INDEX'", "from './index.js'")

    if content != original_content:
        with open(filepath, 'w') as f:
            f.write(content)
        return True
    return False

count = 0
for root, dirs, files in os.walk('core/src'):
    for file in files:
        if file.endswith('.ts') and not file.endswith('.test.ts'):
            if fix_file(os.path.join(root, file)):
                count += 1
print(f"Fixed {count} files.")
