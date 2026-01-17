# Testing Philosophy

## Principles

**Prefer unit tests.** Test small, isolated units of behavior. Fast tests enable fast feedback.

**Avoid mocking and stubbing.** Use real implementations whenever possible. Mocks hide integration bugs and make tests brittle. Only mock when:
- External services (network, filesystem) make tests slow or flaky
- You literally cannot test the behavior otherwise

**Prefer TDD.** Write the test first, watch it fail, make it pass. This clarifies what you're building before you build it.

## Test Structure

```rust
#[test]
fn descriptive_name_of_behavior() {
    // Setup - all test data and preconditions
    let input = ...;

    // Exercise - the action being tested
    let result = function_under_test(input);

    // Assert - verify the outcome
    assert_eq!(result, expected);
}
```

## What to Test

- Public interfaces, not private implementation details
- Edge cases and error conditions, not just happy paths
- Behavior, not implementation

## Running Tests

```bash
devbox run test    # run all tests
devbox run check   # clippy (catches common mistakes)
```

All tests must pass before committing.
