import { z } from 'zod';

export const SkillFrontMatterSchema = z.object({
  id: z.string().regex(/^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/).optional(),
  name: z.string().min(1),
  version: z.string().min(1),
  description: z.string().min(1),
  triggers: z.array(z.string()),
  requires: z.array(z.string()).optional(),
  conflicts: z.array(z.string()).optional(),
  maxTokens: z.number().optional(),
  category: z.string().optional(),
  tags: z.array(z.string()).optional(),
  'applies-to': z.array(z.string()).optional(),
});

export type SkillFrontMatter = z.infer<typeof SkillFrontMatterSchema>;

export interface SkillDocument extends SkillFrontMatter {
  content: string;
  source: 'bundled' | 'external';
}
