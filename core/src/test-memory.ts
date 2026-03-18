import { MemoryManager } from './memory/manager.js';
import path from 'path';

const memoryManager = new MemoryManager({ basePath: path.resolve(process.cwd(), 'memory') });

async function test() {
  const blocks = await memoryManager.getAllBlocks();
  console.log('Blocks:', JSON.stringify(blocks, null, 2));

  const entry = await memoryManager.getEntry('test-id-1');
  console.log('Entry:', JSON.stringify(entry, null, 2));
}

test().catch(console.error);
