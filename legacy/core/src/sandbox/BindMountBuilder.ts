import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import { PlatformPath } from '../lib/PlatformPath.js';
import { Logger } from '../lib/logger.js';
import type { AgentManifest, ResolvedCapabilities } from '../agents/manifest/types.js';
import type { SpawnRequest } from './types.js';
import type { StorageProviderFactory } from '../storage/StorageProvider.js';
import type { AgentRegistry } from '../agents/registry.service.js';

const logger = new Logger('BindMountBuilder');

export class BindMountBuilder {
  static async buildMounts(
    manifest: AgentManifest,
    request: SpawnRequest,
    caps: ResolvedCapabilities,
    finalInstanceId: string,
    agentName: string,
    containerName: string,
    storageFactory: StorageProviderFactory,
    agentRegistry?: AgentRegistry
  ): Promise<string[]> {
    const binds: string[] = [];

    // 1. Workspace mount (Story 3.3)
    const providerName = manifest.workspace?.provider ?? 'local';
    const provider = storageFactory.getProvider(providerName);
    const workspacePath = request.hostWorkspacePath ?? manifest.workspace?.path;
    const writeAllowed = caps.filesystem?.write ?? caps.fs?.write ?? false;
    const mode = writeAllowed ? 'rw' : 'ro';
    binds.push(provider.getBindMount(finalInstanceId, '/workspace', mode, workspacePath));

    // Write AGENT.yaml to workspace so the agent-runtime can load its manifest.
    // The runtime expects flat format (identity/model/tools at top level), not spec-wrapped.
    if (request.type !== 'mcp-server') {
      const wsInternalPath = provider.getPath(finalInstanceId, workspacePath);
      fs.mkdirSync(wsInternalPath, { recursive: true });
      // Ensure workspace is writable by the non-root agent user (uid 1001) in the container.
      // The bind mount inherits host permissions, so we must chmod the host directory.
      try {
        fs.chmodSync(wsInternalPath, 0o777);
      } catch {
        // Best-effort — may fail on some host filesystems
      }
      const spec = (manifest.spec ?? {}) as Record<string, unknown>;
      // Use manifest.model (which has instance overrides applied by Orchestrator)
      // instead of spec.model (which is the raw template without overrides).
      const flatModel = ((manifest.model as unknown) ?? spec.model ?? {}) as Record<
        string,
        unknown
      >;
      const modelWithDefaults = {
        ...flatModel,
        ...(flatModel.name
          ? {}
          : { name: process.env.DEFAULT_MODEL ?? process.env.LLM_MODEL ?? 'default' }),
      };
      const manifestYaml = yaml.dump({
        apiVersion: manifest.apiVersion ?? 'sera/v1',
        kind: manifest.kind ?? 'Agent',
        metadata: manifest.metadata,
        identity: spec.identity ?? manifest.identity,
        model: modelWithDefaults,
        tools: spec.tools,
        skills: spec.skills,
        intercom: spec.intercom,
        resources: spec.resources,
        workspace: spec.workspace,
        memory: spec.memory,
        capabilities: spec.capabilities,
        sandboxBoundary: spec.sandboxBoundary,
        contextFiles: spec.contextFiles,
        notes: spec.notes,
      });
      fs.writeFileSync(path.join(wsInternalPath, 'AGENT.yaml'), manifestYaml, 'utf-8');
      logger.debug(`Wrote AGENT.yaml to workspace for ${containerName}`);
    }

    // 2. Memory mount (Story 3.3)
    const memoryHostDir = process.env.HOST_MEMORY_DIR ?? '/memory';
    const memoryHostPath = PlatformPath.normalizeDockerBindPath(
      `${memoryHostDir}/${finalInstanceId}`
    );
    fs.mkdirSync(memoryHostPath, { recursive: true });
    binds.push(`${memoryHostPath}:/memory:rw`);

    // 3. Knowledge mounts (Story 3.3)
    const knowledgeHostDir = process.env.HOST_KNOWLEDGE_DIR ?? '/knowledge';
    const personalPath = PlatformPath.normalizeDockerBindPath(
      `${knowledgeHostDir}/agents/${agentName}`
    );
    fs.mkdirSync(personalPath, { recursive: true });
    binds.push(`${personalPath}:/knowledge/personal:ro`);

    const sharedPath = PlatformPath.normalizeDockerBindPath(`${knowledgeHostDir}/shared`);
    fs.mkdirSync(sharedPath, { recursive: true });
    binds.push(`${sharedPath}:/knowledge/shared:ro`);

    // 4. MCP Custom Mounts (Story 7.3)
    if (request.type === 'mcp-server' && manifest.mounts) {
      for (const m of manifest.mounts) {
        const mode = m.mode === 'rw' ? 'rw' : 'ro';
        binds.push(`${m.hostPath}:${m.containerPath}:${mode}`);
      }
    }

    // 5. Skill package mounts (M6.2 — capability-filtered)
    const skillPackages = caps.skillPackages;
    if (skillPackages && skillPackages.length > 0) {
      const skillsHostDir = process.env.HOST_SKILLS_DIR ?? '/skills';
      for (const pkg of skillPackages) {
        const pkgHostPath = PlatformPath.normalizeDockerBindPath(`${skillsHostDir}/${pkg}`);
        if (fs.existsSync(pkgHostPath)) {
          binds.push(`${pkgHostPath}:/sera/skills/${pkg}:ro`);
          logger.debug(`Skill mount: ${pkg} for ${containerName}`);
        } else {
          logger.warn(`Skill package "${pkg}" not found at ${pkgHostPath} — skipping mount`);
        }
      }
    }

    // 6. Persistent filesystem grants (Story 3.10)
    if (agentRegistry) {
      try {
        const grants = await agentRegistry.getActiveFilesystemGrants(finalInstanceId);
        for (const grant of grants) {
          if (grant.grant_type === 'persistent') {
            // Canonicalise to prevent path traversal
            const grantPath = fs.existsSync(grant.value)
              ? fs.realpathSync(grant.value)
              : grant.value;
            // host path = container path = grant value (rw access)
            binds.push(`${grantPath}:${grantPath}:rw`);
            logger.info(
              `Persistent grant bind mount: ${grantPath} for instance ${finalInstanceId}`
            );
          }
        }
      } catch (err: unknown) {
        logger.error('Failed to load persistent filesystem grants:', err);
      }
    }

    return binds;
  }
}
