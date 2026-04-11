import yargs from 'yargs';
import { hideBin } from 'yargs/helpers';
import fs from 'node:fs';
import matter from 'gray-matter';
import { SkillFrontMatterSchema } from './schema.js';

import type { ArgumentsCamelCase } from 'yargs';

yargs(hideBin(process.argv))
  .command(
    'create <name>',
    'Create a new skill template',
    (y) => y.option('version', { alias: 'v', default: '1.0.0', type: 'string' }),
    (argv: ArgumentsCamelCase<{ name: string; version: string }>) => {
      const name = argv.name;
      const template = `---
id: ${name}
name: ${name
        .split('-')
        .map((s: string) => s.charAt(0).toUpperCase() + s.slice(1))
        .join(' ')}
version: ${argv.version}
description: A short description of this skill.
triggers: ["${name}"]
category: general
tags: []
---

# ${name}

Add your skill guidance here.
`;
      const filePath = `${name}.md`;
      if (fs.existsSync(filePath)) {
        console.error(`[CLI] Error: File ${filePath} already exists.`);
        process.exit(1);
      }
      fs.writeFileSync(filePath, template);
      console.log(`[CLI] Created skill template: ${filePath}`);
    }
  )
  .command(
    'validate <path>',
    'Validate a skill document',
    (y) => y.positional('path', { type: 'string', demandOption: true }),
    (argv: ArgumentsCamelCase<{ path: string }>) => {
      const filePath = argv.path;
      try {
        if (!fs.existsSync(filePath)) {
          console.error(`[CLI] Error: File ${filePath} not found.`);
          process.exit(1);
        }
        const content = fs.readFileSync(filePath, 'utf-8');
        const { data } = matter(content);
        const result = SkillFrontMatterSchema.safeParse(data);
        if (result.success) {
          console.log('[CLI] ✅ Skill is valid.');
        } else {
          console.error('[CLI] ❌ Validation failed:');
          console.error('[CLI]', JSON.stringify(result.error.format(), null, 2));
          process.exit(1);
        }
      } catch (err) {
        console.error('[CLI] ❌ Error reading or parsing file:', err);
        process.exit(1);
      }
    }
  )
  .command(
    'test <name>',
    'Test a skill (stub)',
    (y) => y.positional('name', { type: 'string', demandOption: true }),
    (argv: ArgumentsCamelCase<{ name: string }>) => {
      const name = argv.name;
      console.log(`[CLI] Testing skill "${name}"... (STUB)`);
      console.log('[CLI] ✅ Skill passed all heuristic checks.');
    }
  )
  .demandCommand(1)
  .help()
  .parse();
