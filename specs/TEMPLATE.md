# Spec Guidelines

These are guidelines, not a rigid template. Shape each spec to fit its content.

## What Every Spec Needs

**User-facing behavior** — What can the user do when this is complete? Be concrete.

**Acceptance criteria** — Checkboxes that define done. For multi-slice specs, group criteria by slice.

**Technical constraints** — Architecture decisions, patterns to follow, things to avoid.

**Error cases** — How should the system behave when things go wrong? Be comprehensive.

## Optional Sections

**Out of scope** — Include when boundaries might be unclear to prevent scope creep.

**Dependencies** — If this spec depends on another spec being complete.

## Slices

A slice is the smallest unit of work that delivers user value. When a spec has multiple slices:

- Each slice should be completable in one agent session
- Group acceptance criteria under slice headings
- Slices can have dependencies on each other
- Prefer slices that are **core** (central to the concept), **small** (completable quickly), and **novel** (reduce uncertainty)

## Example Structure

```markdown
# Feature Name

One-line description of what the user can do.

## Slice 1: [Name]

### User Behavior
What the user experiences.

### Acceptance Criteria
- [ ] Criterion one
- [ ] Criterion two

### Technical Constraints
How to build it.

### Error Cases
- When X happens, show Y
- When Z fails, do W

## Slice 2: [Name]
(depends on Slice 1)

...
```

## Living Document

Specs evolve. Agents may update specs as they learn—adding discovered requirements, refining criteria, or documenting blockers.
