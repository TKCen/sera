import { MemoryManager } from './manager.js';
import fs from 'fs/promises';

export async function testMemory() {
  console.log('--- Starting Memory System Test ---');
  const manager = new MemoryManager();

  // Test 1: Archiving
  console.log('Test 1: Archiving a memory...');
  const title = 'Test Archival Entry';
  const content = 'This is a test content for archival memory systems.';
  const tags = ['test', 'archival', 'sera'];

  const savedPath = await manager.archive(title, content, tags);
  console.log(`Memory saved to: ${savedPath}`);

  // Test 2: File exists and format is correct
  console.log('Test 2: Verifying file content and format...');
  const fileContent = await fs.readFile(savedPath, 'utf8');
  console.log('File Content:\n', fileContent);
  
  if (fileContent.includes('tags:') && fileContent.includes(content)) {
    console.log('✅ File format verification passed.');
  } else {
    console.log('❌ File format verification failed.');
  }

  // Test 3: Retrieval
  console.log('Test 3: Retrieving by title...');
  const retrieved = await manager.getFromArchival(title);
  if (retrieved && retrieved.content === content) {
    console.log('✅ Retrieval passed.');
  } else {
    console.log('❌ Retrieval failed.');
  }

  // Test 4: Search
  console.log('Test 4: Searching memory...');
  const searchResults = await manager.searchArchival({ query: 'test' });
  console.log(`Found ${searchResults.length} results.`);
  if (searchResults.length > 0) {
    console.log('✅ Search passed.');
  } else {
    console.log('❌ Search failed.');
  }

  // Test 5: Working Memory
  console.log('Test 5: Working memory...');
  manager.addToWorkingMemory('Working memory item 1');
  const working = manager.getWorkingMemory();
  if (working.context.includes('Working memory item 1')) {
    console.log('✅ Working memory passed.');
  } else {
    console.log('❌ Working memory failed.');
  }

  console.log('--- Memory System Test Complete ---');
}

if (process.argv[1]?.endsWith('test-memory.ts') || process.argv[1]?.endsWith('test-memory.js')) {
  testMemory().then(() => process.exit(0)).catch(err => {
    console.error(err);
    process.exit(1);
  });
}
