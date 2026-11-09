// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Real-javac and binary-evolution coverage for JVM field linkage rules.

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, VMError, Value, VirtualMachine};
use std::process::Command;
use test_utils::{
    add_integer_constant_value, assert_uncaught_exception, duplicate_constant_value_attribute,
    patch_field_access_flags, patch_method_access_flags, patch_utf8_constant, JavaTestProject,
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

fn load(vm: &mut VirtualMachine, project: &JavaTestProject, names: &[&str]) {
    let classes: Vec<_> = names
        .iter()
        .map(|name| {
            (
                name.replace('.', "/"),
                project.load_class_bytes(name).unwrap(),
            )
        })
        .collect();
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

#[test]
fn instance_resolution_errors_precede_null_checks() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "fieldorder",
            "Target",
            "package fieldorder; public class Target { public int value; }",
        ),
        (
            "fieldorder",
            "Caller",
            "package fieldorder; public class Caller { public static int read() { Target target = null; return target.value; } public static void write() { Target target = null; target.value = 1; } }",
        ),
    ]);
    let static_target = patch_field_access_flags(
        old.load_class_bytes("fieldorder.Target").unwrap(),
        "value",
        "I",
        |flags| flags | 0x0008,
    );
    let classes = vec![
        (
            "fieldorder/Caller".into(),
            old.load_class_bytes("fieldorder.Caller").unwrap(),
        ),
        ("fieldorder/Target".into(), static_target),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    for (method, descriptor) in [("read", "()I"), ("write", "()V")] {
        assert_uncaught_exception(
            &vm.invoke_static("fieldorder/Caller", method, descriptor, &[]),
            "java/lang/IncompatibleClassChangeError",
        );
    }

    let replacement = project_with_sources(&[(
        "fieldorder",
        "Target",
        "package fieldorder; public class Target {}",
    )]);
    let classes = vec![
        (
            "fieldorder/Caller".into(),
            old.load_class_bytes("fieldorder.Caller").unwrap(),
        ),
        (
            "fieldorder/Target".into(),
            replacement.load_class_bytes("fieldorder.Target").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("fieldorder/Caller", "read", "()I", &[]),
        "java/lang/NoSuchFieldError",
    );
}

#[test]
fn missing_symbolic_field_owner_surfaces_as_catchable_noclassdeffounderror() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "missingfield",
            "Target",
            "package missingfield; public class Target { public static int value = 1; }",
        ),
        (
            "missingfield",
            "Caller",
            "package missingfield; public class Caller { public static int run() { \
                try { return Target.value; } \
                catch (NoClassDefFoundError expected) { return 42; } } }",
        ),
    ]);
    let caller = patch_utf8_constant(
        project.load_class_bytes("missingfield.Caller").unwrap(),
        "missingfield/Target",
        "missingfield/Absent",
    );
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&[("missingfield/Caller".into(), caller)]);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("missingfield/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn static_instructions_reject_a_field_that_became_instance() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "statickind",
            "Target",
            "package statickind; public class Target { public static int value; }",
        ),
        (
            "statickind",
            "Caller",
            "package statickind; public class Caller { public static int read() { return Target.value; } public static void write() { Target.value = 1; } }",
        ),
    ]);
    let target = patch_field_access_flags(
        project.load_class_bytes("statickind.Target").unwrap(),
        "value",
        "I",
        |flags| flags & !0x0008,
    );
    let classes = vec![
        (
            "statickind/Caller".into(),
            project.load_class_bytes("statickind.Caller").unwrap(),
        ),
        ("statickind/Target".into(), target),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    for (method, descriptor) in [("read", "()I"), ("write", "()V")] {
        assert_uncaught_exception(
            &vm.invoke_static("statickind/Caller", method, descriptor, &[]),
            "java/lang/IncompatibleClassChangeError",
        );
    }
}

#[test]
fn java_handlers_catch_field_linkage_errors() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "catchfield",
            "TargetMissing",
            "package catchfield; public class TargetMissing { public static int value; }",
        ),
        (
            "catchfield",
            "TargetKind",
            "package catchfield; public class TargetKind { public static int value; }",
        ),
        (
            "catchfield",
            "TargetAccess",
            "package catchfield; public class TargetAccess { public static int value; }",
        ),
        (
            "catchfield",
            "Caller",
            "package catchfield; public class Caller { \
                public static int missing() { try { return TargetMissing.value; } catch (NoSuchFieldError expected) { return 41; } } \
                public static int wrongKind() { try { return TargetKind.value; } catch (IncompatibleClassChangeError expected) { return 42; } } \
                public static int denied() { try { return TargetAccess.value; } catch (IllegalAccessError expected) { return 43; } } }",
        ),
    ]);
    let missing = project_with_sources(&[(
        "catchfield",
        "TargetMissing",
        "package catchfield; public class TargetMissing {}",
    )]);
    let wrong_kind = patch_field_access_flags(
        old.load_class_bytes("catchfield.TargetKind").unwrap(),
        "value",
        "I",
        |flags| flags & !0x0008,
    );
    let denied = patch_field_access_flags(
        old.load_class_bytes("catchfield.TargetAccess").unwrap(),
        "value",
        "I",
        |flags| (flags & !0x0001) | 0x0002,
    );
    let classes = vec![
        (
            "catchfield/Caller".into(),
            old.load_class_bytes("catchfield.Caller").unwrap(),
        ),
        (
            "catchfield/TargetMissing".into(),
            missing
                .load_class_bytes("catchfield.TargetMissing")
                .unwrap(),
        ),
        ("catchfield/TargetKind".into(), wrong_kind),
        ("catchfield/TargetAccess".into(), denied),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    for (method, value) in [("missing", 41), ("wrongKind", 42), ("denied", 43)] {
        assert_eq!(
            vm.invoke_static("catchfield/Caller", method, "()I", &[])
                .unwrap(),
            Value::Int(value)
        );
    }
}

#[test]
fn field_access_and_protected_receiver_rules_survive_binary_evolution() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "accessbase",
            "Base",
            "package accessbase; public class Base { public int value = 42; }",
        ),
        (
            "accesscaller",
            "Child",
            "package accesscaller; public class Child extends accessbase.Base { public static int run(accessbase.Base receiver) { return receiver.value; } public static int call() { return run(new accessbase.Base()); } public static int nullCall() { return run(null); } }",
        ),
    ]);
    let base = patch_field_access_flags(
        project.load_class_bytes("accessbase.Base").unwrap(),
        "value",
        "I",
        |flags| (flags & !0x0001) | 0x0004,
    );
    let classes = vec![
        (
            "accesscaller/Child".into(),
            project.load_class_bytes("accesscaller.Child").unwrap(),
        ),
        ("accessbase/Base".into(), base),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    for method in ["call", "nullCall"] {
        assert_uncaught_exception(
            &vm.invoke_static("accesscaller/Child", method, "()I", &[]),
            "java/lang/IllegalAccessError",
        );
    }
}

#[test]
fn final_field_writes_are_restricted_to_the_declaring_initializer() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "finalwrite",
        "Target",
        "package finalwrite; public class Target { public int value; int mutate() { return value = 42; } public static int run() { return new Target().mutate(); } }",
    )]);
    let target = patch_field_access_flags(
        project.load_class_bytes("finalwrite.Target").unwrap(),
        "value",
        "I",
        |flags| flags | 0x0010,
    );
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("finalwrite/Target", &target)
        .unwrap();
    assert_uncaught_exception(
        &vm.invoke_static("finalwrite/Target", "run", "()I", &[]),
        "java/lang/IllegalAccessError",
    );

    let legal = project_with_sources(&[(
        "finalwrite",
        "Legal",
        "package finalwrite; public class Legal { final int value; Legal() { value = 42; } public static int run() { return new Legal().value; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes(
        "finalwrite/Legal",
        &legal.load_class_bytes("finalwrite.Legal").unwrap(),
    )
    .unwrap();
    assert_eq!(
        vm.invoke_static("finalwrite/Legal", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn inherited_static_field_initializes_only_its_declaring_class() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        ("movefield", "Parent", "package movefield; public class Parent {}"),
        (
            "movefield",
            "Child",
            "package movefield; public class Child extends Parent { public static int value = 1; }",
        ),
        (
            "movefield",
            "Caller",
            "package movefield; public class Caller { public static int run() { return Child.value; } }",
        ),
    ]);
    let replacement = project_with_sources(&[
        (
            "movefield",
            "Parent",
            "package movefield; public class Parent { public static int value = 41; }",
        ),
        (
            "movefield",
            "Child",
            "package movefield; public class Child extends Parent { static { value = 99; } }",
        ),
    ]);
    let classes = vec![
        (
            "movefield/Caller".into(),
            old.load_class_bytes("movefield.Caller").unwrap(),
        ),
        (
            "movefield/Child".into(),
            replacement.load_class_bytes("movefield.Child").unwrap(),
        ),
        (
            "movefield/Parent".into(),
            replacement.load_class_bytes("movefield.Parent").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("movefield/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(41)
    );
}

#[test]
fn field_resolution_searches_direct_interfaces_before_the_superclass() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "interfaceorder",
            "Parent",
            "package interfaceorder; public class Parent {}",
        ),
        (
            "interfaceorder",
            "Child",
            "package interfaceorder; public class Child extends Parent { public static int value = 1; }",
        ),
        (
            "interfaceorder",
            "Caller",
            "package interfaceorder; public class Caller { public static int run() { return Child.value; } }",
        ),
    ]);
    let replacement = project_with_sources(&[
        (
            "interfaceorder",
            "I",
            "package interfaceorder; public interface I { int value = Integer.parseInt(\"42\"); }",
        ),
        (
            "interfaceorder",
            "Parent",
            "package interfaceorder; public class Parent { public static int value = 7; }",
        ),
        (
            "interfaceorder",
            "Child",
            "package interfaceorder; public class Child extends Parent implements I {}",
        ),
    ]);
    let classes = vec![
        (
            "interfaceorder/Caller".into(),
            old.load_class_bytes("interfaceorder.Caller").unwrap(),
        ),
        (
            "interfaceorder/Child".into(),
            replacement
                .load_class_bytes("interfaceorder.Child")
                .unwrap(),
        ),
        (
            "interfaceorder/Parent".into(),
            replacement
                .load_class_bytes("interfaceorder.Parent")
                .unwrap(),
        ),
        (
            "interfaceorder/I".into(),
            replacement.load_class_bytes("interfaceorder.I").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("interfaceorder/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn field_resolution_handles_a_deep_binary_evolution_chain_iteratively() {
    if !javac_available() {
        return;
    }
    const DEPTH: usize = 128;
    let old = project_with_sources(&[
        (
            "deepfield",
            "C127",
            "package deepfield; public class C127 { public static int value = 1; }",
        ),
        (
            "deepfield",
            "Caller",
            "package deepfield; public class Caller { public static int run() { return C127.value; } }",
        ),
    ]);
    let replacement = JavaTestProject::new().unwrap();
    replacement
        .write_source(
            "deepfield",
            "C0",
            "package deepfield; public class C0 { public static int value = 42; }",
        )
        .unwrap();
    for index in 1..DEPTH {
        replacement
            .write_source(
                "deepfield",
                &format!("C{index}"),
                &format!(
                    "package deepfield; public class C{index} extends C{} {{}}",
                    index - 1
                ),
            )
            .unwrap();
    }
    replacement.compile().unwrap();

    let mut classes = vec![(
        "deepfield/Caller".into(),
        old.load_class_bytes("deepfield.Caller").unwrap(),
    )];
    for index in (0..DEPTH).rev() {
        classes.push((
            format!("deepfield/C{index}"),
            replacement
                .load_class_bytes(&format!("deepfield.C{index}"))
                .unwrap(),
        ));
    }
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("deepfield/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn static_storage_uses_the_complete_field_identity() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "fieldidentity",
            "Target",
            "package fieldidentity; public class Target { public static int a = 7; public static long x = 4; }",
        ),
        (
            "fieldidentity",
            "Caller",
            "package fieldidentity; public class Caller { public static long run() { return (long) Target.a * 10L + Target.x; } }",
        ),
    ]);
    let classes = vec![
        (
            "fieldidentity/Caller".into(),
            patch_utf8_constant(
                project.load_class_bytes("fieldidentity.Caller").unwrap(),
                "a",
                "x",
            ),
        ),
        (
            "fieldidentity/Target".into(),
            patch_utf8_constant(
                project.load_class_bytes("fieldidentity.Target").unwrap(),
                "a",
                "x",
            ),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("fieldidentity/Caller", "run", "()J", &[])
            .unwrap(),
        Value::Long(74)
    );
}

#[test]
fn field_resolution_never_falls_back_by_name_across_descriptors() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "descriptorchange",
            "Target",
            "package descriptorchange; public class Target { public static int value; }",
        ),
        (
            "descriptorchange",
            "Caller",
            "package descriptorchange; public class Caller { public static int run() { return Target.value; } }",
        ),
    ]);
    let replacement = project_with_sources(&[(
        "descriptorchange",
        "Target",
        "package descriptorchange; public class Target { public static long value; }",
    )]);
    let classes = vec![
        (
            "descriptorchange/Caller".into(),
            old.load_class_bytes("descriptorchange.Caller").unwrap(),
        ),
        (
            "descriptorchange/Target".into(),
            replacement
                .load_class_bytes("descriptorchange.Target")
                .unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_uncaught_exception(
        &vm.invoke_static("descriptorchange/Caller", "run", "()I", &[]),
        "java/lang/NoSuchFieldError",
    );
}

#[test]
fn primitive_instance_fields_have_jvm_default_values() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "defaults",
        "Target",
        "package defaults; public class Target { int i; long l; double d; Object o; public static int run() { Target t = new Target(); return t.i == 0 && t.l == 0L && t.d == 0.0 && t.o == null ? 42 : -1; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["defaults.Target"]);
    assert_eq!(
        vm.invoke_static("defaults/Target", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn hidden_parent_and_child_fields_keep_distinct_slots() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "hiddenfield",
            "Parent",
            "package hiddenfield; public class Parent { public int value = 1; }",
        ),
        (
            "hiddenfield",
            "Child",
            "package hiddenfield; public class Child extends Parent { public int value = 2; public static int run() { Child c = new Child(); Parent p = c; c.value = 3; p.value = 4; return c.value * 10 + p.value; } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &["hiddenfield.Child", "hiddenfield.Parent"],
    );
    assert_eq!(
        vm.invoke_static("hiddenfield/Child", "run", "()I", &[])
            .unwrap(),
        Value::Int(34)
    );
}

#[test]
fn nestmates_can_access_private_fields() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "fieldnest",
        "Outer",
        "package fieldnest; public class Outer { private int value = 42; static class Inner { static int read(Outer outer) { return outer.value; } } public static int run() { return Inner.read(new Outer()); } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &["fieldnest.Outer$Inner", "fieldnest.Outer"],
    );
    assert_eq!(
        vm.invoke_static("fieldnest/Outer", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn constantvalue_is_prepared_into_the_canonical_static_slot() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "fieldconstant",
            "Target",
            "package fieldconstant; public class Target { public static int value = 1; }",
        ),
        (
            "fieldconstant",
            "Caller",
            "package fieldconstant; public class Caller { public static int run() { return Target.value; } }",
        ),
    ]);
    let replacement = project_with_sources(&[(
        "fieldconstant",
        "Target",
        "package fieldconstant; public class Target { public static final int value = 42; }",
    )]);
    let classes = vec![
        (
            "fieldconstant/Caller".into(),
            old.load_class_bytes("fieldconstant.Caller").unwrap(),
        ),
        (
            "fieldconstant/Target".into(),
            replacement
                .load_class_bytes("fieldconstant.Target")
                .unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("fieldconstant/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn getstatic_initializes_even_after_field_becomes_a_constant_variable() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "constantactive",
            "Target",
            "package constantactive; public class Target { public static int value = 42; }",
        ),
        (
            "constantactive",
            "Sink",
            "package constantactive; public class Sink { public static int hit; }",
        ),
        (
            "constantactive",
            "Caller",
            "package constantactive; public class Caller { public static int run() { return Target.value + 100 * Sink.hit; } }",
        ),
    ]);
    let replacement = project_with_sources(&[
        (
            "constantactive",
            "Target",
            "package constantactive; public class Target { public static final int value = 42; static { Sink.hit = 1; } }",
        ),
        (
            "constantactive",
            "Sink",
            "package constantactive; public class Sink { public static int hit; }",
        ),
    ]);
    let classes = vec![
        (
            "constantactive/Caller".into(),
            old.load_class_bytes("constantactive.Caller").unwrap(),
        ),
        (
            "constantactive/Target".into(),
            replacement
                .load_class_bytes("constantactive.Target")
                .unwrap(),
        ),
        (
            "constantactive/Sink".into(),
            replacement.load_class_bytes("constantactive.Sink").unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("constantactive/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(142)
    );
}

#[test]
fn declaring_clinit_can_overwrite_a_prepared_constantvalue_slot() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "constantoverwrite",
            "Target",
            "package constantoverwrite; public class Target { static final int DUMMY = 41; public static int value = 42; public static void touch() {} }",
        ),
        (
            "constantoverwrite",
            "Caller",
            "package constantoverwrite; public class Caller { public static int run() { Target.touch(); return Target.value; } }",
        ),
    ]);
    let target = patch_field_access_flags(
        project
            .load_class_bytes("constantoverwrite.Target")
            .unwrap(),
        "value",
        "I",
        |flags| flags | 0x0010,
    );
    let target = add_integer_constant_value(target, "value", "I", 41);
    let classes = vec![
        (
            "constantoverwrite/Caller".into(),
            project
                .load_class_bytes("constantoverwrite.Caller")
                .unwrap(),
        ),
        ("constantoverwrite/Target".into(), target),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("constantoverwrite/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn non_native_string_constantvalue_is_prepared() {
    if !javac_available() {
        return;
    }
    let old = project_with_sources(&[
        (
            "stringconstant",
            "Target",
            "package stringconstant; public class Target { public static String value = \"old\"; }",
        ),
        (
            "stringconstant",
            "Caller",
            "package stringconstant; public class Caller { public static int run() { return Target.value.length(); } }",
        ),
    ]);
    let replacement = project_with_sources(&[(
        "stringconstant",
        "Target",
        "package stringconstant; public class Target { public static final String value = \"hello\"; }",
    )]);
    let classes = vec![
        (
            "stringconstant/Caller".into(),
            old.load_class_bytes("stringconstant.Caller").unwrap(),
        ),
        (
            "stringconstant/Target".into(),
            replacement
                .load_class_bytes("stringconstant.Target")
                .unwrap(),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("stringconstant/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(5)
    );
}

#[test]
fn malformed_constantvalue_metadata_is_rejected_during_loading() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "badconstant",
        "Target",
        "package badconstant; public class Target { public static final int value = 42; }",
    )]);
    let original = project.load_class_bytes("badconstant.Target").unwrap();
    let duplicate = duplicate_constant_value_attribute(original.clone(), "value", "I");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badconstant/Target", &duplicate),
        Err(VMError::ClassFormatError(_))
    ));

    let mismatched = patch_utf8_constant(original, "I", "J");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badconstant/Target", &mismatched),
        Err(VMError::ClassFormatError(_))
    ));
}

#[test]
fn malformed_member_names_and_descriptors_fail_before_publication() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "badfield",
        "Target",
        "package badfield; public class Target { public int value; }",
    )]);
    let original = project.load_class_bytes("badfield.Target").unwrap();

    // cafebabe historically accepted a valid descriptor prefix and discarded
    // the suffix.  Linkage must inspect the raw field_info UTF-8 before that
    // normalization can turn malformed input into an apparently valid field.
    let malformed_descriptor = patch_utf8_constant(original.clone(), "I", "IJ");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badfield/Target", &malformed_descriptor),
        Err(VMError::ClassFormatError(_))
    ));
    assert!(matches!(
        vm.invoke_static("badfield/Target", "missing", "()V", &[]),
        Err(VMError::ClassNotFound(_))
    ));

    // Field names use unqualified-name syntax; a slash is always invalid.
    let malformed_name = patch_utf8_constant(original.clone(), "value", "bad/name");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badfield/Target", &malformed_name),
        Err(VMError::ClassFormatError(_))
    ));
    assert!(matches!(
        vm.invoke_static("badfield/Target", "missing", "()V", &[]),
        Err(VMError::ClassNotFound(_))
    ));

    let malformed_method_descriptor = patch_utf8_constant(original.clone(), "()V", "()VI");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badfield/Target", &malformed_method_descriptor),
        Err(VMError::ClassFormatError(_))
    ));

    let malformed_method_name = patch_utf8_constant(original, "<init>", "bad/name");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("badfield/Target", &malformed_method_name),
        Err(VMError::ClassFormatError(_))
    ));
}

#[test]
fn raw_method_descriptors_enforce_the_255_parameter_slot_limit() {
    if !javac_available() {
        return;
    }
    let parameters = |count: usize| {
        (0..count)
            .map(|index| format!("int p{index}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let project = project_with_sources(&[
        (
            "slotlimit",
            "StaticBoundary",
            &format!(
                "package slotlimit; public class StaticBoundary {{ public static void boundary({}) {{}} }}",
                parameters(255)
            ),
        ),
        (
            "slotlimit",
            "InstanceBoundary",
            &format!(
                "package slotlimit; public class InstanceBoundary {{ public void boundary({}) {{}} }}",
                parameters(254)
            ),
        ),
    ]);

    let static_boundary = project
        .load_class_bytes("slotlimit.StaticBoundary")
        .unwrap();
    let instance_boundary = project
        .load_class_bytes("slotlimit.InstanceBoundary")
        .unwrap();
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("slotlimit/StaticBoundary", &static_boundary)
        .unwrap();
    vm.load_class_from_bytes("slotlimit/InstanceBoundary", &instance_boundary)
        .unwrap();

    let descriptor = format!("({})V", "I".repeat(255));
    let forged_instance =
        patch_method_access_flags(static_boundary, "boundary", &descriptor, |flags| {
            flags & !0x0008
        });
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    assert!(matches!(
        vm.load_class_from_bytes("slotlimit/StaticBoundary", &forged_instance),
        Err(VMError::ClassFormatError(_))
    ));
}

#[test]
fn failed_superclass_initialization_marks_the_subclass_erroneous() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "initfailure",
            "Parent",
            "package initfailure; public class Parent { static int broken = 1 / Integer.parseInt(\"0\"); }",
        ),
        (
            "initfailure",
            "Child",
            "package initfailure; public class Child extends Parent { public static int value = 42; }",
        ),
        (
            "initfailure",
            "Caller",
            "package initfailure; public class Caller { \
                public static int first() { try { return Child.value; } \
                    catch (RuntimeException wrong) { return -1; } \
                    catch (ExceptionInInitializerError expected) { return 42; } } \
                public static int later() { try { return Child.value; } \
                    catch (NoClassDefFoundError expected) { return 43; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &[
            "initfailure.Caller",
            "initfailure.Child",
            "initfailure.Parent",
        ],
    );
    assert_eq!(
        vm.invoke_static("initfailure/Caller", "first", "()I", &[])
            .unwrap(),
        Value::Int(42),
        "the caller must see EIIE, never the underlying RuntimeException"
    );
    assert_eq!(
        vm.invoke_static("initfailure/Caller", "later", "()I", &[])
            .unwrap(),
        Value::Int(43),
        "subsequent active use must throw catchable NoClassDefFoundError"
    );
}

#[test]
fn initializer_error_subclasses_are_not_double_wrapped() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "initerror",
            "Target",
            "package initerror; public class Target { static int fail = fail(); static int fail() { throw new Error(); } public static int value = 1; }",
        ),
        (
            "initerror",
            "Caller",
            "package initerror; public class Caller { public static int run() { try { return Target.value; } catch (ExceptionInInitializerError wrong) { return -1; } catch (Error expected) { return 42; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["initerror.Caller", "initerror.Target"]);
    assert_eq!(
        vm.invoke_static("initerror/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn explicit_runtime_exception_from_clinit_is_wrapped_before_caller_handlers() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "initthrow",
            "Target",
            "package initthrow; public class Target { static int value = explode(); static int explode() { throw new RuntimeException(); } }",
        ),
        (
            "initthrow",
            "Caller",
            "package initthrow; public class Caller { \
                public static int first() { try { return Target.value; } \
                    catch (RuntimeException wrong) { return -1; } \
                    catch (ExceptionInInitializerError expected) { return 42; } } \
                public static int later() { try { return Target.value; } \
                    catch (NoClassDefFoundError expected) { return 43; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["initthrow.Caller", "initthrow.Target"]);
    assert_eq!(
        vm.invoke_static("initthrow/Caller", "first", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        vm.invoke_static("initthrow/Caller", "later", "()I", &[])
            .unwrap(),
        Value::Int(43)
    );
}

#[test]
fn initializer_wrapper_preserves_original_exception_identity_and_message() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "initcause",
            "Sink",
            "package initcause; public class Sink { public static RuntimeException expected; public static ExceptionInInitializerError first; }",
        ),
        (
            "initcause",
            "Target",
            "package initcause; public class Target { static { Sink.expected = new RuntimeException(\"boom\"); if (true) throw Sink.expected; } public static int value; }",
        ),
        (
            "initcause",
            "Caller",
            "package initcause; public class Caller { public static int first() { try { return Target.value; } catch (ExceptionInInitializerError e) { Sink.first = e; return e.getException() == Sink.expected && e.getException() == e.getCause() && \"boom\".equals(e.getException().getMessage()) ? 42 : -1; } } public static int later() { try { return Target.value; } catch (NoClassDefFoundError e) { return e.getCause() == Sink.first ? 43 : -2; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &["initcause.Caller", "initcause.Target", "initcause.Sink"],
    );
    assert_eq!(
        vm.invoke_static("initcause/Caller", "first", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        vm.invoke_static("initcause/Caller", "later", "()I", &[])
            .unwrap(),
        Value::Int(43)
    );
}

#[test]
fn vm_generated_and_custom_throwables_have_non_null_stable_identity_and_message() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "throwableidentity",
            "Message",
            "package throwableidentity; public interface Message { String getMessage(); }",
        ),
        (
            "throwableidentity",
            "Custom",
            "package throwableidentity; public class Custom extends RuntimeException implements Message { int marker = 7; public Custom(String message) { super(message); } public String getMessage() { return \"override\"; } public String raw() { return super.getMessage(); } public String toString() { return \"CUSTOM\"; } }",
        ),
        (
            "throwableidentity",
            "Localized",
            "package throwableidentity; public class Localized extends RuntimeException { public Localized() { super(\"raw\"); } public String getLocalizedMessage() { return \"localized\"; } }",
        ),
        (
            "throwableidentity",
            "Caller",
            "package throwableidentity; public class Caller { public static int run() { int score = 0; Object first = null; try { int[] a = new int[0]; return a[1]; } catch (ArrayIndexOutOfBoundsException e) { if (e != null && e.getMessage() != null) { first = e; try { throw e; } catch (ArrayIndexOutOfBoundsException again) { if (again == first) score += 20; } } } Custom custom = new Custom(\"custom\"); Message view = custom; RuntimeException wrapper = new RuntimeException(custom); Localized localized = new Localized(); try { throw custom; } catch (Custom caught) { if (caught == custom && caught.marker == 7 && \"override\".equals(caught.getMessage()) && \"override\".equals(caught.getLocalizedMessage()) && \"override\".equals(view.getMessage()) && \"custom\".equals(caught.raw()) && \"CUSTOM\".equals(wrapper.getMessage()) && \"throwableidentity.Localized: localized\".equals(localized.toString())) score += 22; } return score; } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &[
            "throwableidentity.Caller",
            "throwableidentity.Custom",
            "throwableidentity.Localized",
            "throwableidentity.Message",
        ],
    );
    assert_eq!(
        vm.invoke_static("throwableidentity/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn caller_handler_matches_the_invoke_instruction_when_callee_throws() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "callcatch",
            "Callee",
            "package callcatch; public class Callee { static int boom() { return 1 / Integer.parseInt(\"0\"); } }",
        ),
        (
            "callcatch",
            "Caller",
            "package callcatch; public class Caller { public static int run() { try { return Callee.boom(); } catch (ArithmeticException expected) { return 42; } } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["callcatch.Caller", "callcatch.Callee"]);
    assert_eq!(
        vm.invoke_static("callcatch/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn materialized_array_bounds_messages_report_the_real_length() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "boundsmessage",
        "Caller",
        "package boundsmessage; public class Caller { public static int run() { int score = 0; int[] ints = new int[2]; try { return ints[3]; } catch (ArrayIndexOutOfBoundsException e) { if (\"Index 3 out of bounds for length 2\".equals(e.getMessage())) score += 10; } try { ints[-1] = 7; } catch (ArrayIndexOutOfBoundsException e) { if (\"Index -1 out of bounds for length 2\".equals(e.getMessage())) score += 12; } Object[] refs = new Object[2]; try { return refs[3] == null ? -1 : -2; } catch (ArrayIndexOutOfBoundsException e) { if (\"Index 3 out of bounds for length 2\".equals(e.getMessage())) score += 20; } return score; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["boundsmessage.Caller"]);
    assert_eq!(
        vm.invoke_static("boundsmessage/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn clinit_can_catch_an_exception_escaping_its_helper_method() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "initcaught",
        "Target",
        "package initcaught; public class Target { \
            public static int value; \
            static { try { value = explode(); } catch (ArithmeticException expected) { value = 42; } } \
            static int explode() { return 1 / Integer.parseInt(\"0\"); } \
            public static int read() { return value; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["initcaught.Target"]);
    assert_eq!(
        vm.invoke_static("initcaught/Target", "read", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn class_initialization_uses_jvms_superclass_and_default_interface_order() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "initorder",
            "Log",
            "package initorder; public class Log { static int value; static int record(int digit) { value = value * 10 + digit; return value; } }",
        ),
        (
            "initorder",
            "Top",
            "package initorder; public interface Top { int MARK = Log.record(1); default int top() { return 0; } }",
        ),
        (
            "initorder",
            "Left",
            "package initorder; public interface Left extends Top { int MARK = Log.record(2); default int left() { return 0; } }",
        ),
        (
            "initorder",
            "Right",
            "package initorder; public interface Right extends Top { int MARK = Log.record(4); default int right() { return 0; } }",
        ),
        (
            "initorder",
            "Passive",
            "package initorder; public interface Passive { int MARK = Log.record(9); }",
        ),
        (
            "initorder",
            "Parent",
            "package initorder; public class Parent implements Left, Passive { static { Log.record(3); } }",
        ),
        (
            "initorder",
            "Child",
            "package initorder; public class Child extends Parent implements Right { public static int value = Log.record(5); }",
        ),
        (
            "initorder",
            "Caller",
            "package initorder; public class Caller { public static int run() { return Child.value; } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &project,
        &[
            "initorder.Caller",
            "initorder.Child",
            "initorder.Parent",
            "initorder.Passive",
            "initorder.Right",
            "initorder.Left",
            "initorder.Top",
            "initorder.Log",
        ],
    );
    assert_eq!(
        vm.invoke_static("initorder/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(12345)
    );
}

#[test]
fn direct_vm_entry_initializes_class_before_static_method_body() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "directinit",
            "Sink",
            "package directinit; public class Sink { public static int value; }",
        ),
        (
            "directinit",
            "Target",
            "package directinit; public class Target { static { Sink.value = 42; } public static int run() { return Sink.value; } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["directinit.Target", "directinit.Sink"]);
    assert_eq!(
        vm.invoke_static("directinit/Target", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );
}

#[test]
fn direct_vm_entry_reports_initializer_failure_as_an_uncaught_java_exception() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "directinitfailure",
        "Target",
        "package directinitfailure; public class Target { static { Integer.parseInt(\"boom\"); } public static int run() { return 1; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["directinitfailure.Target"]);
    assert_uncaught_exception(
        &vm.invoke_static("directinitfailure/Target", "run", "()I", &[]),
        "java/lang/ExceptionInInitializerError",
    );
}

#[test]
fn new_initializes_before_constructor_and_rejects_non_instantiable_classes() {
    if !javac_available() {
        return;
    }
    let original = project_with_sources(&[
        (
            "newactive",
            "Target",
            "package newactive; public class Target { static int ready = 42; int seen; public Target() { seen = ready; } }",
        ),
        (
            "newactive",
            "Caller",
            "package newactive; public class Caller { public static int run() { return new Target().seen; } }",
        ),
    ]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(
        &mut vm,
        &original,
        &["newactive.Caller", "newactive.Target"],
    );
    assert_eq!(
        vm.invoke_static("newactive/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(42)
    );

    for replacement_source in [
        "package newactive; public interface Target {}",
        "package newactive; public abstract class Target { public Target() {} }",
    ] {
        let replacement = project_with_sources(&[("newactive", "Target", replacement_source)]);
        let classes = vec![
            (
                "newactive/Caller".into(),
                original.load_class_bytes("newactive.Caller").unwrap(),
            ),
            (
                "newactive/Target".into(),
                replacement.load_class_bytes("newactive.Target").unwrap(),
            ),
        ];
        let mut vm = VirtualMachine::new(MemoryConfig::default());
        let report = vm.load_classes_from_bytes(&classes);
        assert!(report.failures.is_empty(), "{:#?}", report.failures);
        assert_uncaught_exception(
            &vm.invoke_static("newactive/Caller", "run", "()I", &[]),
            "java/lang/InstantiationError",
        );
    }
}

#[test]
fn native_stub_object_fields_have_stable_identity() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[(
        "stubidentity",
        "Caller",
        "package stubidentity; public class Caller { public static int run() { return System.out == System.out && java.awt.Color.RED == java.awt.Color.RED && java.awt.Color.RED == java.awt.Color.red && java.awt.Color.red == java.awt.Color.red ? 1 : 0; } }",
    )]);
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    load(&mut vm, &project, &["stubidentity.Caller"]);
    assert_eq!(
        vm.invoke_static("stubidentity/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(1)
    );
}

#[test]
fn reserved_name_user_class_never_receives_native_stub_constants() {
    if !javac_available() {
        return;
    }
    let project = project_with_sources(&[
        (
            "fake.lang",
            "Integer",
            "package fake.lang; public class Integer { public static int MAX_VALUE = 7; }",
        ),
        (
            "reservedname",
            "Caller",
            "package reservedname; public class Caller { public static int run() { return fake.lang.Integer.MAX_VALUE; } }",
        ),
    ]);
    let classes = vec![
        (
            "reservedname/Caller".into(),
            patch_utf8_constant(
                project.load_class_bytes("reservedname.Caller").unwrap(),
                "fake/lang/Integer",
                "java/lang/Integer",
            ),
        ),
        (
            "java/lang/Integer".into(),
            patch_utf8_constant(
                project.load_class_bytes("fake.lang.Integer").unwrap(),
                "fake/lang/Integer",
                "java/lang/Integer",
            ),
        ),
    ];
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert_eq!(
        vm.invoke_static("reservedname/Caller", "run", "()I", &[])
            .unwrap(),
        Value::Int(7)
    );
}
