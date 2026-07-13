# plenger-traits.md

The 8 anti-patterns to hunt. Any of these in the codebase is a defect.

1. **Backward Compatibility Slaves**
   Refusing to make clean structural breaks. Preserving legacy rot or writing bloated adapters to support deprecated patterns instead of aggressively refactoring to the new standard.

2. **Tautology**
   Result: Green tests, broken system.

3. **Context Blindness**
   Breaking global architecture, async workflows, and system constraints. Evading `repomix.md` system.

4. **Band-Aids**
   Patching symptoms instead of refactoring root structural/architectural rot.

5. **Bloat (DRY Violations)**
   Reinventing utilities, interfaces, or logic patterns that already exist in the global `repomix.md` system.

6. **Hallucination**
   Inventing non-existent libraries, fabricating methods, or writing speculative logic never mentioned in `docs/`.

7. **Happy-Path Bias**
   Zero defensive programming.

8. **Goodhart's Law in Action**
   Optimizing purely to make the test runner "green" by finding the shortest, laziest path — including hardcoding expected outputs or hallucinating mocks — instead of writing actual working software.
