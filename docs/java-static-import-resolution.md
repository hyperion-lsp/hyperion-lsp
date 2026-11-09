# Java static imports and member navigation

Field extraction in `hyperion-types` is shared by workspace indexing, Lombok
metadata, lexical resolution, and nullability. A declaration such as
`public static final int FIRST = 1, VALUE = 7;` retains both identifiers, their
ranges, and their individual array dimensions. Annotation members use ordinary
member extraction, including constants, elements, and nested types.

Bytecode member metadata and symbol identities share canonical descriptor
conversion, retaining qualified parameter and field types. Source attachment
must preserve those identities. Navigation retries an empty result when
workspace bindings advance while retaining a successful, source-proven result
during unrelated publication.

## Generated sources

Indexing a generated file is insufficient without source-set visibility.
Conventional Maven generated main sources are visible to dependent modules;
generated test sources remain local to tests. Custom Maven source locations
and files above the 16 MiB source limit are separate limitations.

Gradle visibility follows modeled source-set roots. The offline Thrift fallback
recognizes assignment, command, and parenthesized `outputDir` forms in balanced
`compileThrift` blocks in project or `buildSrc` Groovy scripts. Comments and
unrelated output settings do not create roots. Source-directory normalization
expands `$projectDir` and `${projectDir}`.

## Validation and troubleshooting

Regression checks assert exact positions for explicit/wildcard static imports,
subsequent field declarators, inherited fields, annotation members, overloads,
and generated-source visibility. Run focused navigation coverage with:

```sh
nice -n 10 cargo test -p hyperion-lsp --test nav -j 4 -- --test-threads=4
nice -n 10 cargo test -p hyperion-semantics --test bytecode_overload_resolution_tests -j 4
```

When reproducing a failure, wait for semantic indexing, check clean diagnostics,
change imports without saving, and verify definition positions again after a
restart. Library and JDK definition targets require matching source archives.
For dependency warnings, inspect the project log to distinguish missing local
metadata from unsupported build modeling; screenshots alone cannot establish
the cause. See [dependency modeling](gradle-dependency-audit.md).
