import { testMemory } from './src/memory/test-memory.js';
testMemory().then(() => process.exit(0)).catch(err => {
    console.error(err);
    process.exit(1);
});
//# sourceMappingURL=test-runner.js.map