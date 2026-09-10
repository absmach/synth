# Parse-error fixtures (twin-pair pattern)

Each `*.synth` here is intentionally invalid and pairs with an expected
diagnostic code (encoded in the filename: `<code>__<short_name>.synth`).
The snapshot test asserts:

1. `parse` returns at least one diagnostic with the named code.
2. That diagnostic has a location with `file == <filename>`.
3. Recovery does not produce more than 2 cascading diagnostics on these
   small fixtures (one underlying mistake → bounded blast radius).

The twin-pair convention from [plan §5.2.3](../../synth_implementation_plan.md):
every error fixture has a passing counterpart under `fixtures/designs/`
that differs by exactly the violation under test.
