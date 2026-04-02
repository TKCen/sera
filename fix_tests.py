import os
import re

def fix_test_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    original_content = content

    # Update mocks to use barrel exports if the target file was updated to use barrel exports
    # Wait, the tests should still mock the internal files if they want,
    # but some tests might be failing because they import the class from index.js
    # and mock the internal file, but the production code is also using index.js.

    # Actually, Vitest mocks often need the exact path.

    # Let's fix the specific failing tests.

    if 'ContextAssembler.test.ts' in filepath:
        content = content.replace("from '../agents/identity/IdentityService.js'", "from '../agents/index.js'")
        content = content.replace("vi.mock('../agents/identity/IdentityService.js')", "vi.mock('../agents/index.js')")
        # And others
        content = content.replace("from '../agents/Orchestrator.js'", "from '../agents/index.js'")
        content = content.replace("vi.mock('../agents/Orchestrator.js')", "vi.mock('../agents/index.js')")
        content = content.replace("from '../agents/AgentFactory.js'", "from '../agents/index.js'")
        content = content.replace("vi.mock('../agents/AgentFactory.js')", "vi.mock('../agents/index.js')")
        content = content.replace("from '../skills/SkillInjector.js'", "from '../skills/index.js'")
        content = content.replace("vi.mock('../skills/SkillInjector.js')", "vi.mock('../skills/index.js')")

    if content != original_content:
        with open(filepath, 'w') as f:
            f.write(content)
        return True
    return False

count = 0
for root, dirs, files in os.walk('core/src'):
    for file in files:
        if file.endswith('.test.ts'):
            if fix_test_file(os.path.join(root, file)):
                count += 1
print(f"Fixed {count} test files.")
