# Security framework patterns

The framework layer contributes web-framework-specific taint sources, sinks
and sanitizers to the scanner's core registries. It keeps those patterns out
of the analysis engine and allows a project to enable only the frameworks it
uses.

## Built-in frameworks

`FrameworkRegistry::with_builtin()` registers and enables:

- Servlet;
- Spring;
- JAX-RS;
- Dropwizard;
- Micronaut;
- Quarkus; and
- Vert.x.

Rebuilding the registry always includes the core, framework-independent
patterns, then merges patterns from each enabled framework. The result is
cached until registration or enablement changes.

```rust,ignore
use hyperion_security::frameworks::FrameworkRegistry;

let mut registry = FrameworkRegistry::with_builtin();
registry.enable_frameworks(&["spring", "servlet"]);

let (sources, sinks, sanitizers) = registry.rebuild_registries();
let request_data = sources.is_source(Some("HttpServletRequest"), "getParameter");
```

`enable_frameworks` replaces the enabled set. Unknown IDs remain in that set
but contribute no patterns; callers that accept external configuration should
validate IDs with `has_framework` first.

## Automatic detection

`FrameworkDetector` scans `pom.xml`, `build.gradle` and `build.gradle.kts`, then
looks for imports and annotations in source files. Dependency, import and
annotation evidence receives confidence 1.0, 0.7 and 0.4 respectively. Evidence
is combined by maximum confidence, and 0.5 is the default enablement threshold.

Detection is deliberately best-effort. It does not evaluate Maven or Gradle,
walks at most 100,000 source files and reads at most 4 MiB from any source
file. A missing result means the detector found insufficient textual evidence,
not that the framework is absent.

## Adding a framework

Implement `Framework`, providing a stable ID, detection patterns and the
source/sink/sanitizer registrations. Register it on an empty registry for a
fully custom set, or add it to `with_builtin()` when it belongs in the shipped
defaults.

Tests should cover:

- dependency, import and annotation detection;
- selective enablement and cache invalidation;
- exact source, sink and sanitizer matches; and
- interaction with the core patterns and at least one other framework.

There is no runtime plugin loader or user configuration format. Adding a new
implementation currently requires compiling it into Hyperion.
