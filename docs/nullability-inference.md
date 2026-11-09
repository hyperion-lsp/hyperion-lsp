# Java nullability inference contract

Hyperion's Java null checker follows the useful parts of TypeScript's strict
null model while respecting Java's annotation ecosystem and runtime defaults.
The goal is a quiet checker: report values that can actually be null, not every
reference whose library author omitted an annotation.

## Model

The checker keeps two facts separate:

- **Declared nullability** is the value's allowed domain. Primitives and
  `@NonNull` references exclude null; `@Nullable` references include it.
  Unannotated Java parameters use a quiet strict assumption inside their own
  body, but remain platform contracts at call sites because Java reference
  types do not encode nullability. Unannotated external bytecode is `Unknown`.
- **Observed nullability** is the value on the current control-flow path. An
  initializer, assignment, guard, assertion, dereference, or method summary can
  narrow the observed state without changing the declaration.

`Unknown` means that Hyperion lacks a contract. It is not evidence that a value
is nullable. Unknown diagnostics are off by default and remain configurable
separately from proven `Null`/`Nullable` diagnostics.

At a control-flow merge, observations are joined. Unreachable paths contribute
nothing. Assignments replace the observation for their target and invalidate
dependent access-path refinements.

## Invariably non-null values

| Source form | Rule | Examples |
|---|---|---|
| Primitive expression | Primitive values cannot be null, including boxed results immediately produced from them | arithmetic, comparison, `instanceof`, unary `!`/`-`, casts from primitives |
| Non-null literal | Every literal except `null` is non-null | strings, text blocks, chars, numbers, booleans |
| Allocation | A normally completed allocation produces a reference | object creation, anonymous classes, arrays, array initializers |
| Runtime object literal | The JVM creates an object for the expression | lambdas, method references, class literals |
| Current receiver | An executing instance method has a receiver | `this`, qualified `Outer.this`, `super` |
| Wrapper | Parentheses preserve the inner state; a reference cast preserves nullability | `(value)`, `(Service) value` |
| Value-producing operator | Java binary expressions produce primitives or a non-null concatenated `String` | `a + b`, `a == b`, `a && b` |
| Conditional value | Non-null only when every reachable arm is non-null | ternary and switch expressions |
| Assignment expression | Has the state of its right-hand value | `current = new Service()` |
| Known contract call | Non-null only for an owner-aware JDK/framework contract or resolved declaration | `Objects.requireNonNull`, `Optional.orElseThrow`, collection factories |

Array and collection element reads are deliberately not assumed non-null.
`array[i]`, `List.get`, `Map.get`, iterator values, generic type variables, and
unannotated external calls stay nullable/unknown unless a stronger contract is
available.

## Parameters and local values

| Case | Required result |
|---|---|
| Primitive parameter/local | `NonNull` |
| Explicit `@NonNull` parameter | `NonNull`; passing a nullable value is diagnosed at the call site |
| Explicit `@Nullable` parameter | `Nullable` until narrowed |
| Unannotated source parameter | Quiet `NonNull` body assumption, but no call-site contract; boundary null checks and intentional null arguments remain valid |
| Local with initializer | State of the initializer |
| Local after assignment | State of the assigned value |
| Local after `x != null`, `Objects.nonNull`, assertion, or throwing precondition | `NonNull` on the surviving path |
| Local after successful dereference | `NonNull`; warn once at the first unsafe dereference |
| Branch/loop merge | Join all reachable incoming states |
| Captured effectively-final local | Retains its narrowed state; mutation invalidates the refinement |

## Class properties

A property is non-null when at least one sound declaration-time guarantee
applies:

- it is primitive;
- it has an explicit non-null or supported injection annotation;
- its declaration initializer is inferred non-null;
- every normally completing constructor definitely assigns a non-null value on
  every path, including constructor delegation; or
- an instance/static initializer definitely assigns it before the corresponding
  class initialization completes.

The following are not sufficient: assignment in only one constructor,
conditional assignment without an `else`, assignment only on an exceptional
path, an unknown right-hand side, or an assignment of `null`. An explicit
`@Nullable` declaration always wins over initialization evidence because the
property may legally be cleared later. Local/parameter shadowing must never
inherit the property's guarantee.

Stable access paths such as `request.user.profile` may be narrowed by an exact
null guard. Reassigning `request`, `request.user`, or the final property
invalidates that refinement. Arbitrary indexed access is narrowed only when the
receiver and index are stable.

Records treat their components like final constructor-assigned properties.
Enum constants, `Class` objects, and the synthetic current instance are
non-null. Nested classes maintain independent field and method summaries.

## Method returns

Return inference is declaration-first and body-informed:

1. Primitive/`void` and explicit `@NonNull` returns are non-null contracts.
2. Explicit `@Nullable` remains nullable regardless of the current body.
3. Otherwise, join every reachable return expression under its converged flow
   state. Throws and unreachable returns do not widen the result.
4. Resolve same-class calls by class, method name, and arity; overloads must not
   contaminate one another.
5. Iterate summaries through delegating and recursive calls to a fixed point.
6. Use indexed cross-file annotations and inferred source summaries when the
   exact owner/signature is known. Unannotated external bytecode stays
   `Unknown`.

Getters inherit the state of the property they return. Ternary, switch,
try/catch, lambda/supplier, and delegation returns join all reachable value
paths. A method that returns non-null on one path and `null` on another is
`Nullable`, never optimistically non-null.

Name-only API whitelists are forbidden: a user method named `getName`, `stream`,
or `collect` may return null. Built-in contracts must be matched against a
resolved owner (and arity where overload behavior differs).

## Flow constructs and invalidation

The checker narrows through `if`/`else`, loops, ternaries, switch arms,
short-circuit `&&`/`||`, negation, early `return`/`throw`, `assert`, and known
throwing preconditions. Joins use only reachable predecessors. `try`/`catch`
includes exceptional paths and `finally` effects.

A write replaces the target's state. A write to an access-path prefix clears
refinements below that prefix. Loop back-edges widen until a fixed point.
Diagnostics are emitted only after convergence so an optimistic intermediate
state cannot create a false redundant-check report.

## Performance contract

- Parse each file once and build each method CFG once per Salsa recomputation.
- Precompute class/property/method summaries and reuse them across method
  analysis; do not rescan the whole syntax tree per expression.
- Drive inter-method inference with a bounded dependency worklist rather than
  rebuilding every method for `method_count * iterations`.
- Keep exact owner/signature and access-path lookups hash-based.
- Preserve Salsa's cached-file fast path.
- Benchmark cold and cached nullability on wide classes, deep call chains, and
  multi-thousand-file projects. Report distributions or Criterion estimates,
  not a single best timing.

These rules intentionally prefer a quiet `Unknown` over an invented guarantee,
and a proven nullable warning over convention-based optimism.

## TypeScript correspondence

The design follows TypeScript's control-flow narrowing, assignment-sensitive
dotted-name guards, and strict property initialization, adapted to Java's
annotations and platform types:

- [Narrowing](https://www.typescriptlang.org/docs/handbook/2/narrowing.html)
  motivates path-sensitive observations and reachable-path joins.
- [TypeScript 2.0 control-flow analysis](https://www.typescriptlang.org/docs/handbook/release-notes/typescript-2-0.html#control-flow-based-type-analysis)
  motivates invalidating a dotted-path refinement after assignment to any of
  its prefixes.
- [`strictPropertyInitialization`](https://www.typescriptlang.org/tsconfig/strictPropertyInitialization.html)
  motivates requiring a declaration initializer or definite assignment on
  every normally completing constructor path.
- [Checker inference notes](https://github.com/microsoft/TypeScript/wiki/Reference-Checker-Inference)
  motivate converging interdependent method summaries instead of relying on
  declaration order.
