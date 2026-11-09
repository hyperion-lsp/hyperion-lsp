# Static member navigation

Navigation selects a type owner from the requesting module's visible source
roots and ordered dependencies. If a library and an unrelated module both
declare `api.Factory`, the requester must see members only from its selected
owner. Combining duplicate classes' members would invent a class that exists
in neither module.

Gradle publishes requester roots and dependency ordering with classpath
refreshes. Type lookup, members, inheritance, and generic projections use the
same immutable scoped view. Refresh tickets and workspace revisions invalidate
stale authority; file semantics use a bounded revision-keyed cache.

Generated directories need known source-set ownership. Indexing a directory
does not prove its module visibility. Custom generated-source declarations are
limited by what the offline parser models.

Argument inference for overloaded static calls uses semantic expression types,
including nested calls, field accesses, and array elements. Local variables,
inherited fields, static imports, and type parameters take precedence over
class-name lookup. Lambda applicability uses candidate functional-interface
metadata; missing metadata and ambiguous candidates remain unresolved. Resolving
an outer lambda call does not guarantee navigation for every expression inside
its body.

Regression fixtures assert exact declaration locations for zero-argument calls,
overloads, nested owners, inheritance, explicit type arguments, shadowing,
ambiguous imports, duplicate types, and ordered project dependencies:

```sh
nice -n 10 cargo test -p hyperion-lsp --test nav static_member_navigation_tests -j 4
nice -n 10 cargo test -p hyperion-lsp --test nav static_argument_navigation_tests -j 4
nice -n 10 cargo test -p hyperion-lsp --test nav lambda_overload_navigation_tests -j 4
nice -n 10 cargo test -p hyperion-lsp --test protocol static_no_arg_navigation -j 4
```
