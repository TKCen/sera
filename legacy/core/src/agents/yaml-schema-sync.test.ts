/**
 * Schema–YAML sync test.
 *
 * Loads every YAML resource from the project's resource directories and
 * validates them against the matching Zod schema. This catches silent drift
 * where the YAML structure diverges from the code's expectations — the kind
 * of bug that only surfaces at runtime when an agent tries to start.
 *
 * Runs as part of `bun run test` — no Docker or database required.
 */

import { describe, it, expect } from 'vitest';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import {
  SandboxBoundarySchema,
  CapabilityPolicySchema,
  NamedListSchema,
  AgentTemplateSchema,
} from './schemas.js';

const PROJECT_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');

function loadYamlFiles(dir: string): { name: string; data: unknown }[] {
  const fullDir = path.join(PROJECT_ROOT, dir);
  if (!fs.existsSync(fullDir)) return [];
  return fs
    .readdirSync(fullDir)
    .filter((f) => f.endsWith('.yaml') || f.endsWith('.yml'))
    .map((f) => ({
      name: f,
      data: yaml.load(fs.readFileSync(path.join(fullDir, f), 'utf8')),
    }));
}

describe('YAML ↔ Zod schema sync', () => {
  describe('sandbox-boundaries/', () => {
    const files = loadYamlFiles('sandbox-boundaries');

    it('has at least one boundary file', () => {
      expect(files.length).toBeGreaterThan(0);
    });

    for (const file of files) {
      it(`${file.name} validates against SandboxBoundarySchema`, () => {
        const result = SandboxBoundarySchema.safeParse(file.data);
        if (!result.success) {
          // Pretty-print the validation errors for easy debugging
          const formatted = result.error.issues
            .map((i) => `  ${i.path.join('.')}: ${i.message}`)
            .join('\n');
          expect.fail(`${file.name} failed schema validation:\n${formatted}`);
        }
      });
    }
  });

  describe('capability-policies/', () => {
    const files = loadYamlFiles('capability-policies');

    it('validates all policy files (if any exist)', () => {
      for (const file of files) {
        const result = CapabilityPolicySchema.safeParse(file.data);
        if (!result.success) {
          const formatted = result.error.issues
            .map((i) => `  ${i.path.join('.')}: ${i.message}`)
            .join('\n');
          expect.fail(`${file.name} failed schema validation:\n${formatted}`);
        }
      }
    });
  });

  describe('lists/', () => {
    const listTypes = [
      'network-allowlist',
      'network-denylist',
      'command-allowlist',
      'command-denylist',
      'secret-list',
    ];

    for (const type of listTypes) {
      const files = loadYamlFiles(`lists/${type}`);
      for (const file of files) {
        it(`lists/${type}/${file.name} validates against NamedListSchema`, () => {
          const result = NamedListSchema.safeParse(file.data);
          if (!result.success) {
            const formatted = result.error.issues
              .map((i) => `  ${i.path.join('.')}: ${i.message}`)
              .join('\n');
            expect.fail(`${file.name} failed schema validation:\n${formatted}`);
          }
        });
      }
    }
  });

  describe('templates/', () => {
    const builtinFiles = loadYamlFiles('templates/builtin');
    const customFiles = loadYamlFiles('templates/custom');
    const allFiles = [...builtinFiles, ...customFiles];

    it('validates all template files (if any exist)', () => {
      for (const file of allFiles) {
        const result = AgentTemplateSchema.safeParse(file.data);
        if (!result.success) {
          const formatted = result.error.issues
            .map((i) => `  ${i.path.join('.')}: ${i.message}`)
            .join('\n');
          expect.fail(`${file.name} failed schema validation:\n${formatted}`);
        }
      }
    });
  });
});
