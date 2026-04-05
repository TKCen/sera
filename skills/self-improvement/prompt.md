# Self-Improvement Analysis

You are performing a structured self-improvement analysis. This is a deliberate, evidence-based review of your recent performance and an opportunity to act on what you learn.

## Step 1: Recover Recent Context

Use `knowledge-query` to retrieve:
- Task completions and outcomes from the last `{{lookback_hours}}` hours
- Any stored insights, errors, or correction notes from recent sessions
- Your current capability gaps or known weaknesses (scope: personal)

## Step 2: Analyze Against Scope

Focus on **{{scope}}** (or all areas if "all"):

### Skills
- Which skills did you use? Were any invoked incorrectly or produced poor results?
- Are there skills you needed but don't have?
- Did you repeat a similar pattern that could be turned into a reusable skill?

### Knowledge
- What did you learn that isn't yet stored?
- Are there stored facts that turned out to be wrong or outdated?
- What would have been most useful to know at the start?

### Tools
- Did any tools fail, time out, or return unexpected results?
- Are there tool usage patterns you should document?
- Were there workarounds you had to use due to tool limitations?

### Behavior
- Did you make any errors in reasoning or judgment?
- Were there moments of unnecessary uncertainty that slowed you down?
- Did you communicate clearly and at the right level of detail?

## Step 3: Identify Top 3 Improvements

Pick the **3 most concrete, actionable improvements** you can make. Prioritize:
1. Things you can fix or act on right now (highest value)
2. Things to document for future sessions
3. Systemic patterns worth escalating

For each improvement:
- **What**: Specific behavior or gap to address
- **Why**: Evidence from recent activity
- **Action**: What you will do about it (store knowledge, update a skill, change a behavior pattern)

## Step 4: Act on Trivial Improvements

For any improvement that can be acted on immediately (e.g., storing a knowledge note, correcting a wrong fact, updating a skill prompt):
- Do it now using the appropriate tool
- Log what you did with `knowledge-store` (type: "action", scope: "personal", tags: ["inner-life/self-improvement"])

## Step 5: Store the Analysis

Store your full analysis using `knowledge-store`:
- type: "insight"
- scope: "personal"
- tags: ["inner-life/self-improvement", "{{scope}}"]
- Include: date, top 3 improvements, actions taken, anything deferred

## Output

Summarize what you found and what you did. Be honest — this is for your own growth, not for an audience.
