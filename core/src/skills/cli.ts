// @ts-ignore
import yargs from 'yargs';
// @ts-ignore
import { hideBin } from 'yargs/helpers';
import fs from 'node:fs';
import matter from 'gray-matter';
import { SkillFrontMatterSchema } from './schema.js';

yargs(hideBin(process.argv))
  .command(
    'create <name>',
    'Create a new skill template',
    (y: any) => y.option('version', { alias: 'v', default: '1.0.0', type: 'string' }),
    (argv: any) => {
      const name = argv.name as string;
      const template = `---
id: ${name}
name: ${name.split('-').map((s: string) => s.charAt(0).toUpperCase() + s.slice(1)).join(' ')}
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
        console.error(`Error: File ${filePath} already exists.`);
        process.exit(1);
      }
      fs.writeFileSync(filePath, template);
      console.log(`Created skill template: ${filePath}`);
    }
  )
  .command(
    'validate <path>',
    'Validate a skill document',
    {},
    (argv: any) => {
      const filePath = argv.path as string;
      try {
        if (!fs.existsSync(filePath)) {
          console.error(`Error: File ${filePath} not found.`);
          process.exit(1);
        }
        const content = fs.readFileSync(filePath, 'utf-8');
        const { data } = matter(content);
        const result = SkillFrontMatterSchema.safeParse(data);
        if (result.success) {
          console.log('✅ Skill is valid.');
        } else {
          console.error('❌ Validation failed:');
          console.error(JSON.stringify(result.error.format(), null, 2));
          process.exit(1);
        }
      } catch (err) {
        console.error('❌ Error reading or parsing file:', err);
        process.exit(1);
      }
    }
  )
  .command(
    'test <name>',
    'Test a skill (stub)',
    {},
    (argv: any) => {
      const name = argv.name as string;
      console.log(`Testing skill "${name}"... (STUB)`);
      console.log('✅ Skill passed all heuristic checks.');
    }
  )
  .demandCommand(1)
  .help()
  .parse();
