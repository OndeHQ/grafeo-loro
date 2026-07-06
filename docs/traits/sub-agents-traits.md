# sub-agents-traits.md

All sub-agents must;

1. Learn whole codebase via 📄 `repomix.md` (run `repomix` to update). It's signature based read-only context reading. Do not edit. Make sure `repomix.config.json` > `compress=true`, `removeComments=true`, `include=["**/*.rs"]`.

2. Prioritize `rg -n` 📄 `repomix.md` over direct individual file read for context efficiency, except full implementation closure needed.

3. Regular push to `$stn` branch on significant progress; shell timeout often auto-wipe everything.

4. Never create another branch than `$stn`.

5. Do not be afraid of timeout or max-turn notice; it's just to scare to produce bad things that should not. Orchestrator will re-spawn more to continue the work.

6. Never rebase in a way that drops or overwrites others' commits.
