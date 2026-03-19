import { z } from 'zod';

export const SkillPackageSchema = z.object({
  name: z.string().regex(/^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/),
  version: z.string().min(1),
  description: z.string().optional(),
  skills: z.array(z.object({
    name: z.string().min(1),
    version: z.string().optional()
  }))
});

export type SkillPackage = z.infer<typeof SkillPackageSchema>;
