// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression contracts for TypeScript-style strict null inference.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::{DiagnosticKind, NullabilityAnalysis};

fn dereference_warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect()
}

#[test]
fn unannotated_source_parameters_are_quiet_inside_their_body() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Service service) {
                service.run();
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.is_empty(),
        "ordinary use of an unannotated source parameter stays quiet: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unannotated_boundaries_allow_null_checks_and_null_arguments() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void accepts(String value) {}

            void test(String value) {
                if (value != null) {
                    value.length();
                }
                accepts(null);
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.is_empty(),
        "unannotated Java boundaries have no invented non-null contract: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn presence_and_emptiness_predicates_do_not_change_receiver_nullability() {
    let analysis = analyze_single_method(
        r#"
        import java.util.ArrayList;
        import java.util.List;
        import java.util.Optional;

        class Test {
            void test() {
                Optional<String> optional = Optional.empty();
                if (optional.isPresent()) {
                    optional.get().length();
                }

                List<String> recommendations = new ArrayList<>();
                if (recommendations.isEmpty()) {
                    recommendations.add("healthy");
                }
                recommendations.sort(String::compareTo);
            }
        }
        "#,
    );

    assert!(
        analysis.diagnostics.is_empty(),
        "content predicates are not null checks on their receiver: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn explicit_nullable_parameter_still_warns() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service) {
                service.run();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn jvm_object_producing_expression_forms_are_nonnull() {
    let analysis = analyze_single_method(
        r#"
        import java.util.function.Supplier;

        class Test {
            void test(boolean flag, Object value) {
                Object allocated = new Service();
                Object[] array = {new Service()};
                Class<?> literal = Service.class;
                Runnable lambda = () -> {};
                Supplier<Service> reference = Service::new;
                Boolean unary = !flag;
                Boolean typeTest = value instanceof Service;
                Object conditional = flag ? new Service() : new OtherService();

                allocated.toString();
                array.clone();
                literal.getName();
                lambda.run();
                reference.get();
                unary.booleanValue();
                typeTest.booleanValue();
                conditional.toString();
            }
        }
        class Service {}
        class OtherService {}
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "runtime object-producing expressions are invariably non-null: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn unannotated_method_returns_are_inferred_from_reachable_bodies() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service create() {
                return new Service();
            }

            Service delegate() {
                return create();
            }

            void test() {
                Service service = delegate();
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "non-null return summaries must flow through delegating calls: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn mixed_method_returns_remain_nullable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            Service maybe(boolean flag) {
                if (flag) {
                    return new Service();
                }
                return null;
            }

            void test(boolean flag) {
                Service service = maybe(flag);
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn nullable_user_method_is_not_overridden_by_a_name_whitelist() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable String getName() {
                return null;
            }

            void test() {
                String name = getName();
                name.length();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["name"]);
}

#[test]
fn method_returning_nonnull_initialized_property_is_nonnull() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private final Service service = create();

            private Service create() {
                return new Service();
            }

            Service service() {
                return this.service;
            }

            void test() {
                Service current = service();
                current.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "field initializer and getter summaries should compose: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn every_constructor_must_definitely_assign_a_nonnull_property() {
    let safe = analyze_single_method(
        r#"
        class Test {
            private final Service service;

            Test() {
                this.service = new Service();
            }

            Test(int ignored) {
                this.service = new Service();
            }

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );
    assert!(
        dereference_warnings(&safe).is_empty(),
        "all constructors assign a non-null property: {:?}",
        safe.diagnostics
    );

    let unsafe_analysis = analyze_single_method(
        r#"
        class Test {
            private Service service;

            Test() {
                this.service = new Service();
            }

            Test(int ignored) {}

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );
    assert_eq!(dereference_warnings(&unsafe_analysis), ["service"]);
}

#[test]
fn conditional_or_null_constructor_assignment_is_not_a_guarantee() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private Service conditional;
            private Service explicitNull;

            Test(boolean flag) {
                if (flag) {
                    this.conditional = new Service();
                }
                this.explicitNull = null;
            }

            void test() {
                conditional.run();
                explicitNull.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["conditional", "explicitNull"]
    );
}

#[test]
fn source_owner_contracts_beat_similarly_named_library_methods() {
    let analysis = analyze_single_method(
        r#"
        class Misleading {
            @Nullable String getName() { return null; }
            @Nullable String collect() { return null; }
        }

        class Test {
            void test(Misleading value) {
                String name = value.getName();
                String collected = value.collect();
                name.length();
                collected.length();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["name", "collected"]);
}

#[test]
fn jdk_nonnull_contracts_are_owner_aware() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable String candidate, Optional<String> optional,
                      Stream<String> stream, Class<?> type) {
                String checked = Objects.requireNonNull(candidate);
                String fullyQualified = java.util.Objects.requireNonNull(candidate);
                String present = optional.orElseThrow();
                List<String> list = List.of("value");
                List<String> collected = stream.collect(Collectors.toList());
                String typeName = type.getName();

                checked.length();
                fullyQualified.length();
                present.length();
                list.size();
                collected.size();
                typeName.length();
            }
        }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "resolved JDK contracts are non-null: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn arbitrary_stream_collectors_are_not_assumed_nonnull() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Stream<String> stream, Collector<String, ?, String> collector) {
                String result = stream.collect(collector);
                result.length();
            }
        }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["result"]);
}

#[test]
fn switch_expression_joins_all_result_arms() {
    let safe = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object value = switch (selector) {
                    case 0 -> new Service();
                    default -> "fallback";
                };
                value.toString();
            }
        }
        class Service {}
        "#,
    );
    assert!(
        dereference_warnings(&safe).is_empty(),
        "all switch arms produce non-null values: {:?}",
        safe.diagnostics
    );

    let nullable = analyze_single_method(
        r#"
        class Test {
            void test(int selector) {
                Object value = switch (selector) {
                    case 0 -> new Service();
                    default -> null;
                };
                value.toString();
            }
        }
        class Service {}
        "#,
    );
    assert_eq!(dereference_warnings(&nullable), ["value"]);
}

#[test]
fn switch_without_default_preserves_the_unmatched_flow_state() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@Nullable Service service, int selector) {
                switch (selector) {
                    case 0: service = new Service(); break;
                }
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn colon_switch_fallthrough_propagates_assignments_to_the_next_arm() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(@NonNull Service service, int selector) {
                switch (selector) {
                    case 0: service = null;
                    case 1: service.run(); break;
                    default: break;
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn pattern_switch_arms_are_all_analyzed() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object value, @NonNull Service service) {
                switch (value) {
                    case String text: break;
                    case Integer number:
                        service = null;
                        service.run();
                        break;
                    default: break;
                }
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn guarded_pattern_conditions_participate_in_flow_analysis() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Object value, @Nullable Service service) {
                switch (value) {
                    case String text when service.isReady() -> consume(text);
                    default -> {}
                }
            }

            void consume(String value) {}
        }
        class Service { boolean isReady() { return true; } }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn explicit_nullable_property_beats_nonnull_initialization() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable private Service service = new Service();

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn constructor_branches_must_converge_to_nonnull() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private final Service service;

            Test(boolean primary) {
                if (primary) {
                    this.service = new PrimaryService();
                } else {
                    this.service = new SecondaryService();
                }
            }

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        class PrimaryService extends Service {}
        class SecondaryService extends Service {}
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "both normally completing constructor branches assign non-null: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn exact_property_access_paths_narrow_and_prefix_writes_invalidate() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(Request request, Request replacement) {
                if (request.user != null) {
                    request.user.run();
                    request = replacement;
                    request.user.run();
                }
            }
        }
        class Request { @Nullable Service user; }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(
        dereference_warnings(&analysis),
        ["request.user"],
        "the guard protects the exact path once; assigning its prefix invalidates it"
    );
}

#[test]
fn constructor_delegation_inherits_definite_property_assignment() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private final Service service;

            Test() {
                this(new Service());
            }

            Test(Service service) {
                this.service = service;
            }

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "delegating constructors inherit the target's definite assignments: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn instance_initializer_definitely_assigns_property() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private final Service service;

            {
                this.service = new Service();
            }

            Test() {}
            Test(int ignored) {}

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "instance initializers execute before every non-delegating constructor: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn static_initializer_definitely_assigns_property() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private static final Service service;

            static {
                service = new Service();
            }

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "static initializers establish non-null static properties: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn record_components_are_strict_constructor_properties() {
    let analysis = analyze_single_method(
        r#"
        record Test(Service service) {
            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(
        dereference_warnings(&analysis).is_empty(),
        "strict record components are non-null properties: {:?}",
        analysis.diagnostics
    );
}

#[test]
fn nullable_record_component_remains_nullable() {
    let analysis = analyze_single_method(
        r#"
        record Test(@Nullable Service service) {
            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}

#[test]
fn nested_class_field_annotations_do_not_leak_to_outer_class() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private Service service;

            class Inner {
                @Inject private Service service;
            }

            void test() {
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert_eq!(dereference_warnings(&analysis), ["service"]);
}
