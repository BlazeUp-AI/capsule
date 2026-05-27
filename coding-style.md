---
description: Project coding style and architecture preferences
---

# Architecture & Style Preferences

- Prefer monolithic file structures — keep related logic together in single files rather than splitting into many tiny modules
- Within monoliths, maintain clean modularity: use clear section separators, well-named functions, and logical grouping
- Prioritize readability: code should read top-to-bottom like a narrative
- Avoid premature abstraction — but abstraction itself is not bad; coupling is the real enemy
- Good abstractions represent real concepts in the system you can name in natural language
- Don't extract commonality just because code looks similar — extract when it represents shared *logic* (a single source of truth)
- "Don't Repeat Yourself" is about logic, not properties/fields or superficial code duplication
- Interfaces exist for consumers, not implementers — don't create an interface just because two things *could* implement it
- An abstraction must earn its keep: if it only saves assigning a variable twice, it's not worth the coupling it introduces
- Abstractions *are* worth it when they enable isolation (testing), deferred execution, or represent many implementations with complex construction
- Design for easy removal: prefer self-contained units (files/modules) that can be deleted cleanly over intermingled config switches
- Never nest more than 3 levels deep — denest via extraction (pull logic into its own function) or inversion (flip conditions to early returns)
- Put the unhappy path first with early returns; keep the happy path at the top level, not buried in nested blocks
- Prefer `if` + early return over `if/else` — flatten the else branch by inverting the condition
- Functions should be focused but files can be long if they're well-organized
- Prefer well-defined types and explicit return types — make outcomes clear and predictable
- Favor strong typing over `any` or loose types; if a type can be narrowed, narrow it
- Name things well enough that comments become redundant — code should be self-documenting

# Optimization Philosophy

- Premature optimization is the root of all evil — if the code doesn't work, we don't care how fast it doesn't work
- The development triangle: velocity (speed of shipping), adaptability (flexibility for change), performance (speed + quality)
- Pure velocity = shortest path to feature, but accumulates technical debt that kills future velocity
- Adaptability = reusable/extensible/configurable code that reduces changes needed for new features
- Over-adapting to cases that never happen wastes time and hurts both velocity and quality
- The balance shifts with project stage:
  - Early: maximize velocity
  - Early-middle: high velocity, medium adaptability
  - Middle: maximize adaptability
  - Late-middle: adaptability + performance
  - Final/shipping: maximize performance, some adaptability
