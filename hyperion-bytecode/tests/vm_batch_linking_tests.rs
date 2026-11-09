// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end coverage for staged, order-independent VM class definition.

mod test_utils;

use hyperion_bytecode::interpreter::linker::ClassLinker;
use hyperion_bytecode::interpreter::{ClassLoader, MemoryConfig, VMError, Value, VirtualMachine};
use std::process::Command;
use test_utils::{
    assert_uncaught_exception, patch_method_access_flags, patch_super_class_index,
    patch_utf8_constant, JavaTestProject,
};

fn javac_available() -> bool {
    Command::new("javac")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn project_with_sources(sources: &[(&str, &str, &str)]) -> JavaTestProject {
    let project = JavaTestProject::new().unwrap();
    for (package, name, source) in sources {
        project
            .write_source(&package.replace('.', "/"), name, source)
            .unwrap();
    }
    project.compile().unwrap();
    project
}

#[test]
fn parser_rejects_invalid_superclass_shapes_and_duplicate_fields() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("shape", "Plain", "package shape; public class Plain {}"),
        (
            "shape",
            "Service",
            "package shape; public interface Service {}",
        ),
        (
            "shape",
            "Fields",
            "package shape; public class Fields { int aa; int bb; }",
        ),
    ]);
    let loader = ClassLoader::new();

    for bytes in [
        patch_super_class_index(project.load_class_bytes("shape.Plain").unwrap(), 0),
        patch_super_class_index(project.load_class_bytes("shape.Service").unwrap(), 0),
    ] {
        assert!(matches!(
            ClassLinker::parse_class(&bytes, &loader),
            Err(VMError::ClassFormatError(_))
        ));
    }

    let object_bytes = patch_utf8_constant(
        project.load_class_bytes("shape.Plain").unwrap(),
        "shape/Plain",
        "java/lang/Object",
    );
    assert!(matches!(
        ClassLinker::parse_class(&object_bytes, &loader),
        Err(VMError::ClassFormatError(_))
    ));

    let duplicate_fields = patch_utf8_constant(
        project.load_class_bytes("shape.Fields").unwrap(),
        "bb",
        "aa",
    );
    assert!(matches!(
        ClassLinker::parse_class(&duplicate_fields, &loader),
        Err(VMError::ClassFormatError(_))
    ));
}

#[test]
fn valid_module_descriptor_is_not_misreported_as_malformed_class() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[("", "module-info", "module shape.module { }")]);
    let bytes = project.load_class_bytes("module-info").unwrap();
    assert!(matches!(
        ClassLinker::parse_class(&bytes, &ClassLoader::new()),
        Err(VMError::Unimplemented(_))
    ));
}

#[test]
fn batch_links_dependency_parent_before_an_alphabetically_earlier_child() {
    if !javac_available() {
        eprintln!("skipping batch link test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "dependency",
            "Parent",
            r#"
                package dependency;
                public class Parent {
                    public int inherited = 40;
                }
            "#,
        ),
        (
            "app",
            "Child",
            r#"
                package app;
                public class Child extends dependency.Parent {
                    public int own = 2;
                    public static int answer() {
                        Child child = new Child();
                        return child.inherited + child.own;
                    }
                }
            "#,
        ),
    ]);
    // Deliberately child-first. These labels match classpath discovery's JVM
    // slash form and therefore also exercise slash-to-dot invocation aliases.
    let classes = vec![
        (
            "app/Child".to_string(),
            project.load_class_bytes("app.Child").unwrap(),
        ),
        (
            "dependency/Parent".to_string(),
            project.load_class_bytes("dependency.Parent").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert!(report.is_loaded("app/Child"));
    assert_eq!(
        vm.invoke_static("app/Child", "answer", "()I", &[]).unwrap(),
        Value::Int(42)
    );
}

#[test]
fn later_batch_can_extend_dependency_loaded_by_an_earlier_batch() {
    if !javac_available() {
        eprintln!("skipping batch link test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "dependency",
            "Base",
            r#"
                package dependency;
                public class Base {
                    protected int value = 41;
                }
            "#,
        ),
        (
            "workspace",
            "Derived",
            r#"
                package workspace;
                public final class Derived extends dependency.Base {
                    public static int answer() { return new Derived().value + 1; }
                }
            "#,
        ),
    ]);
    let dependency = vec![(
        "dependency/Base".to_string(),
        project.load_class_bytes("dependency.Base").unwrap(),
    )];
    let workspace = vec![(
        "workspace/Derived".to_string(),
        project.load_class_bytes("workspace.Derived").unwrap(),
    )];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(vm.load_classes_from_bytes(&dependency).failures.is_empty());
    let report = vm.load_classes_from_bytes(&workspace);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("workspace/Derived", "answer", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn abstract_base_and_interface_default_link_independently_of_input_order() {
    if !javac_available() {
        eprintln!("skipping batch link test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "api",
            "Service",
            r#"
                package api;
                public interface Service {
                    int required();
                    default int answer() { return required() + 1; }
                }
            "#,
        ),
        (
            "workspace",
            "AbstractBase",
            r#"
                package workspace;
                public abstract class AbstractBase implements api.Service {
                    protected int value = 41;
                    public int required() { return value; }
                }
            "#,
        ),
        (
            "workspace",
            "Implementation",
            r#"
                package workspace;
                public final class Implementation extends AbstractBase {
                    public static int run() { return new Implementation().answer(); }
                }
            "#,
        ),
    ]);
    let classes = vec![
        (
            "workspace/Implementation".to_string(),
            project
                .load_class_bytes("workspace.Implementation")
                .unwrap(),
        ),
        (
            "workspace/AbstractBase".to_string(),
            project.load_class_bytes("workspace.AbstractBase").unwrap(),
        ),
        (
            "api/Service".to_string(),
            project.load_class_bytes("api.Service").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("workspace/Implementation", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn subclass_inherits_superclass_interface_routes() {
    if !javac_available() {
        eprintln!("skipping inherited-itable test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "api",
            "Answer",
            "package api; public interface Answer { default int value() { return 42; } }",
        ),
        (
            "dependency",
            "Base",
            "package dependency; public class Base implements api.Answer {}",
        ),
        (
            "workspace",
            "Derived",
            "package workspace; public final class Derived extends dependency.Base {}",
        ),
        (
            "workspace",
            "Caller",
            r#"
                package workspace;
                public final class Caller {
                    public static int run() {
                        api.Answer answer = new Derived();
                        return answer.value();
                    }
                }
            "#,
        ),
    ]);
    let classes = vec![
        (
            "workspace/Caller".to_string(),
            project.load_class_bytes("workspace.Caller").unwrap(),
        ),
        (
            "workspace/Derived".to_string(),
            project.load_class_bytes("workspace.Derived").unwrap(),
        ),
        (
            "dependency/Base".to_string(),
            project.load_class_bytes("dependency.Base").unwrap(),
        ),
        (
            "api/Answer".to_string(),
            project.load_class_bytes("api.Answer").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("workspace/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn subinterface_default_retargets_inherited_routes_for_all_symbolic_owners() {
    if !javac_available() {
        eprintln!("skipping subinterface-default route test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "api",
            "I",
            "package api; public interface I { default int value() { return 1; } }",
        ),
        (
            "app",
            "A",
            "package app; public class A implements api.I {}",
        ),
        (
            "api",
            "J",
            "package api; public interface J extends I { default int value() { return 2; } }",
        ),
        (
            "app",
            "B",
            "package app; public class B extends A implements api.J {}",
        ),
        (
            "app",
            "Caller",
            r#"
                package app;
                public final class Caller {
                    public static int run() {
                        B b = new B();
                        A asA = b;
                        api.I asI = b;
                        api.J asJ = b;
                        return b.value() * 1000
                            + asA.value() * 100
                            + asI.value() * 10
                            + asJ.value();
                    }
                }
            "#,
        ),
    ]);
    let names = ["app.Caller", "app.B", "api.J", "app.A", "api.I"];
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("app/Caller", "run", "()I", &[]).unwrap(),
        Value::Int(2222)
    );
}

#[test]
fn missing_parent_only_rejects_its_structural_dependents() {
    if !javac_available() {
        eprintln!("skipping batch link test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "dependency",
            "Omitted",
            "package dependency; public class Omitted {}",
        ),
        (
            "workspace",
            "Broken",
            "package workspace; public class Broken extends dependency.Omitted {}",
        ),
        (
            "workspace",
            "Good",
            "package workspace; public class Good { public static int answer() { return 42; } }",
        ),
    ]);
    let good_bytecode = project.load_class_bytes("workspace.Good").unwrap();
    let classes = vec![
        (
            "workspace/Broken".to_string(),
            project.load_class_bytes("workspace.Broken").unwrap(),
        ),
        ("workspace/WrongName".to_string(), good_bytecode.clone()),
        ("workspace/Good".to_string(), good_bytecode),
        ("workspace/Malformed".to_string(), vec![0xca, 0xfe]),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.is_loaded("workspace.Good"));
    assert!(report.failure_for("workspace/Broken").is_some());
    assert!(report.failure_for("workspace/Malformed").is_some());
    assert!(report.failure_for("workspace/WrongName").is_some());
    assert_eq!(
        vm.invoke_static("workspace/Good", "answer", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn private_interface_helpers_are_not_virtual_or_default_candidates() {
    if !javac_available() {
        eprintln!("skipping private-interface link test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[(
        "api",
        "WithHelper",
        r#"
            package api;
            public interface WithHelper {
                private int helper() { return 41; }
                default int answer() { return helper() + 1; }
            }
        "#,
    )]);
    let loader = ClassLoader::new();
    let class = ClassLinker::parse_class(
        &project.load_class_bytes("api.WithHelper").unwrap(),
        &loader,
    )
    .unwrap();

    assert!(class
        .vtable
        .iter()
        .any(|method| method.name.as_ref() == "answer"));
    assert!(!class
        .vtable
        .iter()
        .any(|method| method.name.as_ref() == "helper"));
}

#[test]
fn binary_incompatible_abstract_interface_dispatch_reports_abstract_method_error() {
    if !javac_available() {
        eprintln!("skipping abstract-interface dispatch test: javac not found on PATH");
        return;
    }

    // Compile consumers against a default method, then replace the interface
    // with a binary-incompatible abstract declaration. A JVM may define the
    // old concrete class; the failure belongs at the invocation site as AME.
    let old_project = project_with_sources(&[
        (
            "api",
            "Service",
            "package api; public interface Service { default int value() { return 42; } }",
        ),
        (
            "workspace",
            "Implementation",
            "package workspace; public final class Implementation implements api.Service {}",
        ),
        (
            "workspace",
            "Caller",
            r#"
                package workspace;
                public final class Caller {
                    public static int run() {
                        api.Service service = new Implementation();
                        return service.value();
                    }
                }
            "#,
        ),
    ]);
    let new_project = project_with_sources(&[(
        "api",
        "Service",
        "package api; public interface Service { int value(); }",
    )]);
    let classes = vec![
        (
            "workspace/Caller".to_string(),
            old_project.load_class_bytes("workspace.Caller").unwrap(),
        ),
        (
            "workspace/Implementation".to_string(),
            old_project
                .load_class_bytes("workspace.Implementation")
                .unwrap(),
        ),
        (
            "api/Service".to_string(),
            new_project.load_class_bytes("api.Service").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("workspace/Caller", "run", "()I", &[]),
        "java/lang/AbstractMethodError",
    );
}

#[test]
fn package_private_routes_split_across_packages_and_resume_in_the_original_package() {
    if !javac_available() {
        eprintln!("skipping package-private dispatch test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "a",
            "Base",
            r#"
                package a;
                public class Base {
                    int value() { return 1; }
                    public int call() { return value(); }
                }
            "#,
        ),
        (
            "b",
            "DirectChild",
            "package b; public class DirectChild extends a.Base { public int value() { return 2; } }",
        ),
        (
            "b",
            "Middle",
            "package b; public class Middle extends a.Base {}",
        ),
        (
            "a",
            "ReturnedChild",
            "package a; public class ReturnedChild extends b.Middle { int value() { return 3; } }",
        ),
        (
            "a",
            "Caller",
            r#"
                package a;
                public final class Caller {
                    public static int run() {
                        b.DirectChild direct = new b.DirectChild();
                        ReturnedChild returned = new ReturnedChild();
                        return direct.call() * 1000
                            + direct.value() * 100
                            + returned.call() * 10
                            + returned.value();
                    }
                }
            "#,
        ),
    ]);
    let names = [
        "a.Caller",
        "a.ReturnedChild",
        "b.Middle",
        "b.DirectChild",
        "a.Base",
    ];
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("a/Caller", "run", "()I", &[]).unwrap(),
        // DirectChild.value is a distinct route, while ReturnedChild is back
        // in Base's runtime package and therefore directly overrides Base's
        // route under JVMS 5.4.5 despite the intermediate package hop.
        Value::Int(1233)
    );
}

#[test]
fn same_package_and_protected_methods_override_the_inherited_slot() {
    if !javac_available() {
        eprintln!("skipping access-aware override test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "same",
            "PackageBase",
            "package same; public class PackageBase { int value() { return 1; } public int call() { return value(); } }",
        ),
        (
            "same",
            "PackageChild",
            "package same; public class PackageChild extends PackageBase { int value() { return 2; } }",
        ),
        (
            "protectedbase",
            "Base",
            "package protectedbase; public class Base { protected int value() { return 3; } public int call() { return value(); } }",
        ),
        (
            "other",
            "ProtectedChild",
            "package other; public class ProtectedChild extends protectedbase.Base { public int value() { return 4; } }",
        ),
        (
            "other",
            "Caller",
            r#"
                package other;
                public final class Caller {
                    public static int run() {
                        return new same.PackageChild().call() * 10
                            + new ProtectedChild().call();
                    }
                }
            "#,
        ),
    ]);
    let names = [
        "other.Caller",
        "other.ProtectedChild",
        "protectedbase.Base",
        "same.PackageChild",
        "same.PackageBase",
    ];
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("other/Caller", "run", "()I", &[]).unwrap(),
        Value::Int(24)
    );
}

#[test]
fn lowered_access_declaration_keeps_a_distinct_symbolic_dispatch_route() {
    if !javac_available() {
        eprintln!("skipping lowered-access route test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "a",
            "A",
            "package a; public class A { public int value() { return 1; } public int callA() { return value(); } }",
        ),
        (
            "b",
            "B",
            "package b; public class B extends a.A { public int value() { return 2; } public int callB() { return value(); } }",
        ),
        (
            "c",
            "C",
            "package c; public class C extends b.B { public int value() { return 3; } }",
        ),
        (
            "c",
            "Caller",
            r#"
                package c;
                public final class Caller {
                    public static int run() {
                        b.B b = new b.B();
                        C c = new C();
                        return b.callA() * 100 + c.callA() * 10 + c.callB();
                    }
                }
            "#,
        ),
    ]);
    let patched_b = patch_method_access_flags(
        project.load_class_bytes("b.B").unwrap(),
        "value",
        "()I",
        |flags| flags & !(0x0001 | 0x0002 | 0x0004),
    );
    let classes = vec![
        (
            "c/Caller".to_string(),
            project.load_class_bytes("c.Caller").unwrap(),
        ),
        ("c/C".to_string(), project.load_class_bytes("c.C").unwrap()),
        ("b/B".to_string(), patched_b),
        ("a/A".to_string(), project.load_class_bytes("a.A").unwrap()),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("c/Caller", "run", "()I", &[]).unwrap(),
        // On B, A.value resolves to B.value. On C, A.value resolves to
        // C.value, while B.value still resolves to B.value because C cannot
        // override B's package-private declaration from another package.
        Value::Int(232)
    );
}

#[test]
fn non_public_class_method_suppresses_default_but_invokeinterface_rejects_it() {
    if !javac_available() {
        eprintln!("skipping interface/default access test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "api",
            "Service",
            "package api; public interface Service { default int value() { return 2; } }",
        ),
        (
            "dependency",
            "Base",
            "package dependency; public class Base { int value() { return 1; } public int call() { return value(); } }",
        ),
        (
            "workspace",
            "Child",
            "package workspace; public class Child extends dependency.Base implements api.Service {}",
        ),
        (
            "workspace",
            "Caller",
            r#"
                package workspace;
                public final class Caller {
                    public static int callBase() {
                        return new Child().call();
                    }
                    public static int callInterface() {
                        Child child = new Child();
                        api.Service service = child;
                        return service.value();
                    }
                }
            "#,
        ),
    ]);
    let names = [
        "workspace.Caller",
        "workspace.Child",
        "dependency.Base",
        "api.Service",
    ];
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("workspace/Caller", "callBase", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
    assert_uncaught_exception(
        &vm.invoke_static("workspace/Caller", "callInterface", "()I", &[]),
        "java/lang/IllegalAccessError",
    );
}

#[test]
fn non_public_class_method_selected_for_abstract_interface_is_rejected() {
    if !javac_available() {
        eprintln!("skipping abstract interface access test: javac not found on PATH");
        return;
    }

    let old_project = project_with_sources(&[
        (
            "api",
            "Service",
            "package api; public interface Service { default int value() { return 2; } }",
        ),
        (
            "dependency",
            "Base",
            "package dependency; public class Base { int value() { return 1; } }",
        ),
        (
            "workspace",
            "Child",
            "package workspace; public class Child extends dependency.Base implements api.Service {}",
        ),
        (
            "workspace",
            "Caller",
            "package workspace; public final class Caller { public static int run() { api.Service service = new Child(); return service.value(); } }",
        ),
    ]);
    let new_project = project_with_sources(&[(
        "api",
        "Service",
        "package api; public interface Service { int value(); }",
    )]);
    let classes = vec![
        (
            "workspace/Caller".to_string(),
            old_project.load_class_bytes("workspace.Caller").unwrap(),
        ),
        (
            "workspace/Child".to_string(),
            old_project.load_class_bytes("workspace.Child").unwrap(),
        ),
        (
            "dependency/Base".to_string(),
            old_project.load_class_bytes("dependency.Base").unwrap(),
        ),
        (
            "api/Service".to_string(),
            new_project.load_class_bytes("api.Service").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("workspace/Caller", "run", "()I", &[]),
        "java/lang/IllegalAccessError",
    );
}

#[test]
fn private_parent_method_is_not_a_virtual_override_candidate() {
    if !javac_available() {
        eprintln!("skipping private-method dispatch test: javac not found on PATH");
        return;
    }

    let project = project_with_sources(&[
        (
            "privatebase",
            "Base",
            "package privatebase; public class Base { private int value() { return 1; } public int call() { return value(); } }",
        ),
        (
            "otherprivate",
            "Child",
            "package otherprivate; public class Child extends privatebase.Base { public int value() { return 2; } }",
        ),
        (
            "otherprivate",
            "Caller",
            "package otherprivate; public class Caller { public static int run() { Child child = new Child(); return child.call() * 10 + child.value(); } }",
        ),
    ]);
    let names = [
        "otherprivate.Caller",
        "otherprivate.Child",
        "privatebase.Base",
    ];
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("otherprivate/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(12)
    );
}

#[test]
fn binary_incompatible_override_of_newly_final_method_is_rejected() {
    if !javac_available() {
        eprintln!("skipping final-method compatibility test: javac not found on PATH");
        return;
    }

    let old_project = project_with_sources(&[
        (
            "dependency",
            "Base",
            "package dependency; public class Base { public int value() { return 1; } }",
        ),
        (
            "workspace",
            "Child",
            "package workspace; public class Child extends dependency.Base { public int value() { return 2; } }",
        ),
    ]);
    let new_project = project_with_sources(&[(
        "dependency",
        "Base",
        "package dependency; public class Base { public final int value() { return 1; } }",
    )]);
    let classes = vec![
        (
            "workspace/Child".to_string(),
            old_project.load_class_bytes("workspace.Child").unwrap(),
        ),
        (
            "dependency/Base".to_string(),
            new_project.load_class_bytes("dependency.Base").unwrap(),
        ),
    ];

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.is_loaded("dependency.Base"));
    let failure = report.failure_for("workspace.Child").unwrap();
    assert!(failure.error.contains("final method"), "{failure:#?}");
}
