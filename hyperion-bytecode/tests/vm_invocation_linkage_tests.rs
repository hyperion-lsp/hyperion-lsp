// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Real-javac and binary-evolution coverage for invocation linkage rules.

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use std::process::Command;
use test_utils::{
    assert_uncaught_exception, patch_method_access_flags, patch_method_ref_owner,
    patch_method_ref_tag, JavaTestProject,
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
fn private_interface_helper_is_invoked_exactly_not_as_a_virtual_candidate() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("helpers", "Service", "package helpers; public interface Service { private int helper() { return 41; } default int answer() { return helper() + 1; } }"),
        ("helpers", "Impl", "package helpers; public class Impl implements Service {}"),
        ("helpers", "Caller", "package helpers; public class Caller { public static int run() { return new Impl().answer(); } }"),
    ]);
    let classes: Vec<_> = ["helpers.Caller", "helpers.Impl", "helpers.Service"]
        .into_iter()
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
        vm.invoke_static("helpers/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn nestmate_can_invoke_outer_private_method() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "nest",
        "Outer",
        "package nest; public class Outer { private int secret() { return 41; } static class Inner { int read(Outer outer) { return outer.secret() + 1; } } public static int run() { return new Inner().read(new Outer()); } }",
    )]);
    let classes: Vec<_> = ["nest.Outer$Inner", "nest.Outer"]
        .into_iter()
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
        vm.invoke_static("nest/Outer", "run", "()I", &[]).unwrap(),
        Value::Int(42)
    );
}

#[test]
fn invokevirtual_rejects_an_exact_symbolic_method_that_became_static() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("evolve", "Base", "package evolve; public class Base { public int value() { return 1; } }"),
        ("evolve", "Child", "package evolve; public class Child extends Base { public int value() { return 2; } }"),
        ("evolve", "Caller", "package evolve; public class Caller { public static int run() { return new Child().value(); } }"),
    ]);
    let child = patch_method_access_flags(
        project.load_class_bytes("evolve.Child").unwrap(),
        "value",
        "()I",
        |flags| flags | 0x0008,
    );
    let classes = vec![
        (
            "evolve/Caller".into(),
            project.load_class_bytes("evolve.Caller").unwrap(),
        ),
        ("evolve/Child".into(), child),
        (
            "evolve/Base".into(),
            project.load_class_bytes("evolve.Base").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("evolve/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokevirtual_resolves_static_evolution_before_null_receiver_check() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("nullorder", "Base", "package nullorder; public class Base { public int value() { return 1; } }"),
        ("nullorder", "Caller", "package nullorder; public class Caller { public static int run() { Base value = null; return value.value(); } }"),
    ]);
    let base = patch_method_access_flags(
        project.load_class_bytes("nullorder.Base").unwrap(),
        "value",
        "()I",
        |flags| flags | 0x0008,
    );
    let classes = vec![
        (
            "nullorder/Caller".into(),
            project.load_class_bytes("nullorder.Caller").unwrap(),
        ),
        ("nullorder/Base".into(), base),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("nullorder/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokeinterface_rejects_a_receiver_from_an_incompatible_evolved_class() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("evolveiface", "Service", "package evolveiface; public interface Service { int value(); }"),
        ("evolveiface", "Impl", "package evolveiface; public class Impl implements Service { public int value() { return 42; } }"),
        ("evolveiface", "Caller", "package evolveiface; public class Caller { public static int run() { Service service = new Impl(); return service.value(); } }"),
    ]);
    let replacement = project_with_sources(&[(
        "evolveiface",
        "Impl",
        "package evolveiface; public class Impl { public int value() { return 42; } }",
    )]);
    let classes = vec![
        (
            "evolveiface/Caller".into(),
            old.load_class_bytes("evolveiface.Caller").unwrap(),
        ),
        (
            "evolveiface/Impl".into(),
            replacement.load_class_bytes("evolveiface.Impl").unwrap(),
        ),
        (
            "evolveiface/Service".into(),
            old.load_class_bytes("evolveiface.Service").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("evolveiface/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokeinterface_rejects_an_exact_symbolic_method_that_became_static() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("staticiface", "Service", "package staticiface; public interface Service { default int value() { return 1; } }"),
        ("staticiface", "Impl", "package staticiface; public class Impl implements Service {}"),
        ("staticiface", "Caller", "package staticiface; public class Caller { public static int run() { Service service = new Impl(); return service.value(); } }"),
    ]);
    let service = patch_method_access_flags(
        project.load_class_bytes("staticiface.Service").unwrap(),
        "value",
        "()I",
        |flags| flags | 0x0008,
    );
    let classes = vec![
        (
            "staticiface/Caller".into(),
            project.load_class_bytes("staticiface.Caller").unwrap(),
        ),
        (
            "staticiface/Impl".into(),
            project.load_class_bytes("staticiface.Impl").unwrap(),
        ),
        ("staticiface/Service".into(), service),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("staticiface/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokeinterface_does_not_fall_back_to_a_class_method_when_symbolic_method_was_removed() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("removediface", "Service", "package removediface; public interface Service { int value(); }"),
        ("removediface", "Impl", "package removediface; public class Impl implements Service { public int value() { return 42; } }"),
        ("removediface", "Caller", "package removediface; public class Caller { public static int run() { Service service = new Impl(); return service.value(); } }"),
    ]);
    let replacement = project_with_sources(&[(
        "removediface",
        "Service",
        "package removediface; public interface Service { int other(); }",
    )]);
    let classes = vec![
        (
            "removediface/Caller".into(),
            old.load_class_bytes("removediface.Caller").unwrap(),
        ),
        (
            "removediface/Impl".into(),
            old.load_class_bytes("removediface.Impl").unwrap(),
        ),
        (
            "removediface/Service".into(),
            replacement
                .load_class_bytes("removediface.Service")
                .unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("removediface/Caller", "run", "()I", &[]),
        "java/lang/NoSuchMethodError",
    );
}

#[test]
fn invokeinterface_reports_selected_access_error_before_abstract_method_error() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("precedence", "Service", "package precedence; public interface Service { default int value() { return 1; } }"),
        ("precedence", "Base", "package precedence; public class Base {}"),
        ("precedence", "Child", "package precedence; public class Child extends Base implements Service {}"),
        ("otherprecedence", "Caller", "package otherprecedence; public class Caller { public static int run() { precedence.Service service = new precedence.Child(); return service.value(); } }"),
    ]);
    let replacement = project_with_sources(&[
        (
            "precedence",
            "Base",
            "package precedence; public abstract class Base { abstract int value(); }",
        ),
        (
            "precedence",
            "Service",
            "package precedence; public interface Service { int value(); }",
        ),
    ]);
    let classes = vec![
        (
            "otherprecedence/Caller".into(),
            old.load_class_bytes("otherprecedence.Caller").unwrap(),
        ),
        (
            "precedence/Child".into(),
            old.load_class_bytes("precedence.Child").unwrap(),
        ),
        (
            "precedence/Base".into(),
            replacement.load_class_bytes("precedence.Base").unwrap(),
        ),
        (
            "precedence/Service".into(),
            replacement.load_class_bytes("precedence.Service").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("otherprecedence/Caller", "run", "()I", &[]),
        "java/lang/IllegalAccessError",
    );
}

#[test]
fn cross_package_protected_call_on_this_uses_the_runtime_receiver_qualifier() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("protectedbase", "Base", "package protectedbase; public class Base { protected int value() { return 42; } }"),
        ("protectedchild", "Child", "package protectedchild; public class Child extends protectedbase.Base { public int run() { return this.value(); } }"),
        ("protectedchild", "Caller", "package protectedchild; public class Caller { public static int run() { return new Child().run(); } }"),
    ]);
    let classes: Vec<_> = [
        "protectedchild.Caller",
        "protectedchild.Child",
        "protectedbase.Base",
    ]
    .into_iter()
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
        vm.invoke_static("protectedchild/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn invokeinterface_count_uses_jvm_slots_for_long_and_double() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("wide", "Service", "package wide; public interface Service { long add(long left, double right); }"),
        ("wide", "Impl", "package wide; public class Impl implements Service { public long add(long left, double right) { return left + (long) right; } }"),
        ("wide", "Caller", "package wide; public class Caller { public static long run() { Service service = new Impl(); return service.add(40L, 2.0); } }"),
    ]);
    let classes: Vec<_> = ["wide.Caller", "wide.Impl", "wide.Service"]
        .into_iter()
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
        vm.invoke_static("wide/Caller", "run", "()J", &[]).unwrap(),
        Value::Long(42)
    );
}

#[test]
fn invokevirtual_resolves_child_instance_before_static_super_declaration() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("perclass", "Base", "package perclass; public class Base { public int value() { return 1; } }"),
        ("perclass", "Child", "package perclass; public class Child extends Base { public int value() { return 42; } }"),
        ("perclass", "Caller", "package perclass; public class Caller { public static int run() { return new Child().value(); } }"),
    ]);
    let base = patch_method_access_flags(
        project.load_class_bytes("perclass.Base").unwrap(),
        "value",
        "()I",
        |flags| flags | 0x0008,
    );
    let classes = vec![
        (
            "perclass/Caller".into(),
            project.load_class_bytes("perclass.Caller").unwrap(),
        ),
        (
            "perclass/Child".into(),
            project.load_class_bytes("perclass.Child").unwrap(),
        ),
        ("perclass/Base".into(), base),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("perclass/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn invokestatic_rejects_a_symbolic_method_that_evolved_to_instance() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("staticevolve", "Target", "package staticevolve; public class Target { public static int value() { return 42; } }"),
        ("staticevolve", "Caller", "package staticevolve; public class Caller { public static int run() { return Target.value(); } }"),
    ]);
    let target = patch_method_access_flags(
        project.load_class_bytes("staticevolve.Target").unwrap(),
        "value",
        "()I",
        |flags| flags & !0x0008,
    );
    let classes = vec![
        (
            "staticevolve/Caller".into(),
            project.load_class_bytes("staticevolve.Caller").unwrap(),
        ),
        ("staticevolve/Target".into(), target),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("staticevolve/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokestatic_rejects_an_interface_methodref_tag_for_a_class_owner() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "statictag",
            "Target",
            "package statictag; public class Target { public static int value() { return 42; } }",
        ),
        (
            "statictag",
            "Caller",
            "package statictag; public class Caller { public static int run() { return Target.value(); } }",
        ),
    ]);
    let caller = patch_method_ref_tag(
        project.load_class_bytes("statictag.Caller").unwrap(),
        "statictag/Target",
        "value",
        "()I",
        11,
    );
    let classes = vec![
        ("statictag/Caller".into(), caller),
        (
            "statictag/Target".into(),
            project.load_class_bytes("statictag.Target").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("statictag/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokespecial_acc_super_selects_a_new_direct_superclass_override() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("accsuper", "A", "package accsuper; public class A { public int value() { return 1; } }"),
        ("accsuper", "B", "package accsuper; public class B extends A {}"),
        ("accsuper", "C", "package accsuper; public class C extends B { public int run() { return super.value(); } public static Class<?> marker() { return A.class; } }"),
        ("accsuper", "Caller", "package accsuper; public class Caller { public static int run() { return new C().run(); } }"),
    ]);
    let evolved = project_with_sources(&[
        (
            "accsuper",
            "A",
            "package accsuper; public class A { public int value() { return 1; } }",
        ),
        (
            "accsuper",
            "B",
            "package accsuper; public class B extends A { public int value() { return 42; } }",
        ),
    ]);
    let child = patch_method_ref_owner(
        old.load_class_bytes("accsuper.C").unwrap(),
        "accsuper/B",
        "value",
        "()I",
        "accsuper/A",
    );
    let classes = vec![
        (
            "accsuper/Caller".into(),
            old.load_class_bytes("accsuper.Caller").unwrap(),
        ),
        ("accsuper/C".into(), child),
        (
            "accsuper/B".into(),
            evolved.load_class_bytes("accsuper.B").unwrap(),
        ),
        (
            "accsuper/A".into(),
            evolved.load_class_bytes("accsuper.A").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("accsuper/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn invokespecial_rejects_an_unrelated_interface_owner() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        ("specialiface", "I", "package specialiface; public interface I { default int value() { return 42; } }"),
        ("specialiface", "K", "package specialiface; public interface K { default int value() { return 7; } }"),
        ("specialiface", "C", "package specialiface; public class C implements I { public int run() { return I.super.value(); } public static Class<?> marker() { return K.class; } }"),
        ("specialiface", "Caller", "package specialiface; public class Caller { public static int run() { return new C().run(); } }"),
    ]);
    let child = patch_method_ref_owner(
        project.load_class_bytes("specialiface.C").unwrap(),
        "specialiface/I",
        "value",
        "()I",
        "specialiface/K",
    );
    let classes = vec![
        (
            "specialiface/Caller".into(),
            project.load_class_bytes("specialiface.Caller").unwrap(),
        ),
        ("specialiface/C".into(), child),
        (
            "specialiface/I".into(),
            project.load_class_bytes("specialiface.I").unwrap(),
        ),
        (
            "specialiface/K".into(),
            project.load_class_bytes("specialiface.K").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("specialiface/Caller", "run", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
}

#[test]
fn invokespecial_rejects_an_abstract_selected_target_before_execution() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "specialabstract",
            "Base",
            "package specialabstract; public class Base { public int value() { return 1; } }",
        ),
        (
            "specialabstract",
            "Child",
            "package specialabstract; public class Child extends Base { public int run() { return super.value(); } }",
        ),
        (
            "specialabstract",
            "Caller",
            "package specialabstract; public class Caller { public static int run() { return new Child().run(); } }",
        ),
    ]);
    let evolved = project_with_sources(&[(
        "specialabstract",
        "Base",
        "package specialabstract; public abstract class Base { public abstract int value(); }",
    )]);
    let classes = vec![
        (
            "specialabstract/Caller".into(),
            old.load_class_bytes("specialabstract.Caller").unwrap(),
        ),
        (
            "specialabstract/Child".into(),
            old.load_class_bytes("specialabstract.Child").unwrap(),
        ),
        (
            "specialabstract/Base".into(),
            evolved.load_class_bytes("specialabstract.Base").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("specialabstract/Caller", "run", "()I", &[]),
        "java/lang/AbstractMethodError",
    );
}

#[test]
fn array_clone_resolves_before_primitive_and_reference_fast_paths() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "arrayclone",
        "Caller",
        "package arrayclone; public class Caller { public static int run() { int[] a = { 1, 2 }; int[] b = a.clone(); String[] x = { \"x\" }; String[] y = x.clone(); b[0] = 40; return b[0] + b[1] + y[0].length() - 1; } }",
    )]);
    let classes = vec![(
        "arrayclone/Caller".into(),
        project.load_class_bytes("arrayclone.Caller").unwrap(),
    )];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("arrayclone/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn conflicting_defaults_fail_at_invocation_but_a_subclass_override_resolves_them() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("defaults", "I", "package defaults; public interface I { default int value() { return 1; } }"),
        ("defaults", "J", "package defaults; public interface J extends I { default int value() { return 2; } }"),
        ("defaults", "B", "package defaults; public class B implements I, J {}"),
        ("defaults", "Child", "package defaults; public class Child extends B { public int value() { return 42; } }"),
        ("defaults", "Caller", "package defaults; public class Caller { public static int viaB() { return new B().value(); } public static int viaI() { I x = new B(); return x.value(); } public static int viaJ() { J x = new B(); return x.value(); } public static int child() { return new Child().value(); } }"),
    ]);
    let evolved = project_with_sources(&[
        (
            "defaults",
            "I",
            "package defaults; public interface I { default int value() { return 1; } }",
        ),
        (
            "defaults",
            "J",
            "package defaults; public interface J { default int value() { return 2; } }",
        ),
    ]);
    let classes = vec![
        (
            "defaults/Caller".into(),
            old.load_class_bytes("defaults.Caller").unwrap(),
        ),
        (
            "defaults/Child".into(),
            old.load_class_bytes("defaults.Child").unwrap(),
        ),
        (
            "defaults/B".into(),
            old.load_class_bytes("defaults.B").unwrap(),
        ),
        (
            "defaults/I".into(),
            evolved.load_class_bytes("defaults.I").unwrap(),
        ),
        (
            "defaults/J".into(),
            evolved.load_class_bytes("defaults.J").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    for method in ["viaB", "viaI", "viaJ"] {
        assert_uncaught_exception(
            &vm.invoke_static("defaults/Caller", method, "()I", &[]),
            "java/lang/IncompatibleClassChangeError",
        );
    }
    assert_eq!(
        vm.invoke_static("defaults/Caller", "child", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn subclass_default_selection_keeps_the_superclass_effective_interface_closure() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("closure", "I", "package closure; public interface I { default int value() { return 1; } }"),
        ("closure", "K", "package closure; public interface K extends I { default int value() { return 2; } }"),
        ("closure", "J", "package closure; public interface J extends I { }"),
        ("closure", "R", "package closure; public interface R extends I, K { default int value() { return 4; } }"),
        ("closure", "B", "package closure; public class B implements I, K {}"),
        ("closure", "Mid", "package closure; public class Mid extends B {}"),
        ("closure", "C", "package closure; public class C extends Mid implements J {}"),
        ("closure", "D", "package closure; public class D extends B implements R {}"),
        ("closure", "Caller", "package closure; public class Caller { public static int conflict() { return new C().value(); } public static int resolved() { return new D().value(); } }"),
    ]);
    let evolved = project_with_sources(&[
        ("closure", "I", "package closure; public interface I { default int value() { return 1; } }"),
        ("closure", "K", "package closure; public interface K { default int value() { return 2; } }"),
        ("closure", "J", "package closure; public interface J extends I { default int value() { return 3; } }"),
        ("closure", "R", "package closure; public interface R extends I, K { default int value() { return 4; } }"),
    ]);
    let classes: Vec<_> = ["Caller", "D", "C", "Mid", "B"]
        .into_iter()
        .map(|name| {
            (
                format!("closure/{name}"),
                old.load_class_bytes(&format!("closure.{name}")).unwrap(),
            )
        })
        .chain(["R", "J", "K", "I"].into_iter().map(|name| {
            (
                format!("closure/{name}"),
                evolved
                    .load_class_bytes(&format!("closure.{name}"))
                    .unwrap(),
            )
        }))
        .collect();
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("closure/Caller", "conflict", "()I", &[]),
        "java/lang/IncompatibleClassChangeError",
    );
    assert_eq!(
        vm.invoke_static("closure/Caller", "resolved", "()I", &[])
            .unwrap(),
        Value::Int(4)
    );
}

#[test]
fn missing_symbolic_method_owner_is_a_catchable_linkage_error() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "missingowner",
            "Missing",
            "package missingowner; public class Missing { public static void call() {} }",
        ),
        (
            "missingowner",
            "Caller",
            "package missingowner; public class Caller { public static int run() { try { Missing.call(); return -1; } catch (NoClassDefFoundError expected) { return 42; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes(
        "missingowner/Caller",
        &project.load_class_bytes("missingowner.Caller").unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.invoke_static("missingowner/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn missing_exception_table_type_replaces_the_original_and_reaches_the_invoker() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "missingcatch",
            "Missing",
            "package missingcatch; public class Missing extends RuntimeException {}",
        ),
        (
            "missingcatch",
            "Inner",
            "package missingcatch; public class Inner { static void run() { try { int ignored = 1 / Integer.parseInt(\"0\"); } catch (Missing ignored) {} } }",
        ),
        (
            "missingcatch",
            "Caller",
            "package missingcatch; public class Caller { public static int run() { try { Inner.run(); return -1; } catch (NoClassDefFoundError expected) { return 42; } } }",
        ),
    ]);
    let classes = ["missingcatch.Caller", "missingcatch.Inner"].map(|name| {
        (
            name.replace('.', "/"),
            project.load_class_bytes(name).unwrap(),
        )
    });
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("missingcatch/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}
