// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Class bytecode parsing and linking

use super::bootstrap::{
    BootstrapArg, MethodHandleKind, ResolvedBootstrapMethod, ResolvedMethodHandle,
};
use super::errors::{VMError, VMResult};
use super::frame::ExceptionTableEntry;
use super::method_area::{
    ConstantFieldValue, FieldConstantValue, FieldDeclaration, FieldLayout, LoadedClass,
    NestMetadata, NonVirtualMethodDeclaration,
};
use super::verifier::verify_bytecode;
use cafebabe::parse_class;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Class linker - parses bytecode and creates LoadedClass metadata
pub struct ClassLinker;

impl ClassLinker {
    /// Parse bytecode and create a LoadedClass
    pub fn parse_class(
        bytecode: &[u8],
        _class_loader: &super::class_loader::ClassLoader,
    ) -> VMResult<LoadedClass> {
        // cafebabe validates names, but its descriptor parser accepts a valid
        // prefix and silently ignores trailing bytes. Validate raw field_info
        // and method_info metadata before normalization loses that data.
        validate_raw_member_metadata(bytecode)?;
        let class_file =
            parse_class(bytecode).map_err(|e| VMError::ClassFormatError(format!("{:?}", e)))?;

        // Get class name (convert internal form java/lang/Object to external java.lang.Object)
        let class_name = class_file.this_class.to_string().replace('/', ".");
        let is_interface = class_file
            .access_flags
            .contains(cafebabe::ClassAccessFlags::INTERFACE);
        if class_file.access_flags.bits() & 0x8000 != 0 {
            if class_name != "module-info"
                || class_file.super_class.is_some()
                || !class_file.interfaces.is_empty()
                || !class_file.fields.is_empty()
                || !class_file.methods.is_empty()
            {
                return Err(VMError::ClassFormatError(
                    "invalid ACC_MODULE classfile shape".to_string(),
                ));
            }
            return Err(VMError::Unimplemented(
                "module descriptors are metadata, not executable classes".to_string(),
            ));
        }
        match (class_name.as_str(), class_file.super_class.as_ref()) {
            ("java.lang.Object", None) => {}
            ("java.lang.Object", Some(parent)) => {
                return Err(VMError::ClassFormatError(format!(
                    "java.lang.Object must not declare superclass {parent}"
                )));
            }
            (_, None) => {
                return Err(VMError::ClassFormatError(format!(
                    "class {class_name} must declare a superclass"
                )));
            }
            (_, Some(parent))
                if is_interface && parent.to_string().replace('/', ".") != "java.lang.Object" =>
            {
                return Err(VMError::ClassFormatError(format!(
                    "interface {class_name} must declare java.lang.Object as its superclass"
                )));
            }
            _ => {}
        }
        let mut class = LoadedClass::new(class_name);

        // Set superclass
        if let Some(ref super_class) = class_file.super_class {
            let superclass_name = super_class.to_string().replace('/', ".");
            class.set_superclass(superclass_name);
        }

        // Set interfaces
        for interface in &class_file.interfaces {
            let interface_name = interface.to_string().replace('/', ".");
            class.add_interface(interface_name);
        }

        let mut field_signatures = HashSet::new();
        let mut declared_fields = Vec::with_capacity(class_file.fields.len());
        let mut constant_field_values = Vec::new();
        for (declaration_index, field) in class_file.fields.iter().enumerate() {
            let signature = (field.name.to_string(), field.descriptor.to_string());
            if !field_signatures.insert(signature.clone()) {
                return Err(VMError::ClassFormatError(format!(
                    "duplicate field {}.{}:{}",
                    class.name, signature.0, signature.1
                )));
            }
            validate_field_modifiers(
                &class.name,
                is_interface,
                &signature.0,
                &signature.1,
                field.access_flags.bits(),
            )?;
            declared_fields.push(FieldDeclaration::new(
                signature.0,
                signature.1,
                field.access_flags.bits(),
            ));
            let constant_attributes: Vec<_> = field
                .attributes
                .iter()
                .filter_map(|attribute| {
                    let cafebabe::attributes::AttributeData::ConstantValue(value) = &attribute.data
                    else {
                        return None;
                    };
                    Some(value)
                })
                .collect();
            if constant_attributes.len() > 1 {
                return Err(VMError::ClassFormatError(format!(
                    "duplicate ConstantValue attribute on {}.{}:{}",
                    class.name, field.name, field.descriptor
                )));
            }
            if let Some(value) = constant_attributes.first().map(|value| match value {
                cafebabe::constant_pool::LiteralConstant::Integer(value) => {
                    FieldConstantValue::Int(*value)
                }
                cafebabe::constant_pool::LiteralConstant::Float(value) => {
                    FieldConstantValue::Float(*value)
                }
                cafebabe::constant_pool::LiteralConstant::Long(value) => {
                    FieldConstantValue::Long(*value)
                }
                cafebabe::constant_pool::LiteralConstant::Double(value) => {
                    FieldConstantValue::Double(*value)
                }
                cafebabe::constant_pool::LiteralConstant::String(value) => {
                    FieldConstantValue::String(Arc::from(value.as_ref()))
                }
                cafebabe::constant_pool::LiteralConstant::StringBytes(value) => {
                    FieldConstantValue::String(Arc::from(
                        String::from_utf8_lossy(value).into_owned(),
                    ))
                }
            }) {
                let descriptor = field.descriptor.to_string();
                if !value.matches_descriptor(&descriptor) {
                    return Err(VMError::ClassFormatError(format!(
                        "ConstantValue type does not match {}.{}:{}",
                        class.name, field.name, descriptor
                    )));
                }
                constant_field_values.push(ConstantFieldValue {
                    declaration_index: declaration_index as u16,
                    value,
                });
            }
        }
        class.declared_fields = declared_fields.into();
        class.constant_field_values = constant_field_values.into();

        // Build field layout for instance fields
        let mut field_layout = FieldLayout::new();
        for field in &class_file.fields {
            if !field
                .access_flags
                .contains(cafebabe::FieldAccessFlags::STATIC)
            {
                let field_name = field.name.to_string();
                field_layout.add_field_with_descriptor(field_name, field.descriptor.to_string());
            }
        }
        class.field_layout = field_layout;

        // Record class kind before constructing declaration metadata so an
        // interface-selected route inherited by a class remains distinguishable
        // from a genuine class declaration.
        class.modifiers = class_file.access_flags.bits();
        class.is_interface = is_interface;

        let mut declared_signatures = HashSet::new();
        for method in &class_file.methods {
            let signature = (method.name.to_string(), method.descriptor.to_string());
            if !declared_signatures.insert(signature.clone()) {
                return Err(VMError::ClassFormatError(format!(
                    "duplicate method {}.{}{}",
                    class.name, signature.0, signature.1
                )));
            }
        }

        // Build virtual method table
        for method in &class_file.methods {
            if !method
                .access_flags
                .contains(cafebabe::MethodAccessFlags::STATIC)
                && !method
                    .access_flags
                    .contains(cafebabe::MethodAccessFlags::PRIVATE)
            {
                let method_name = method.name.to_string();
                if method_name != "<init>" && method_name != "<clinit>" {
                    let descriptor = method.descriptor.to_string();
                    class.add_vtable_entry_with_flags(
                        method_name,
                        descriptor,
                        class.name.clone(),
                        method.access_flags.bits(),
                    );
                }
            }
        }

        for method in &class_file.methods {
            if method.access_flags.intersects(
                cafebabe::MethodAccessFlags::STATIC | cafebabe::MethodAccessFlags::PRIVATE,
            ) || matches!(method.name.to_string().as_str(), "<init>" | "<clinit>")
            {
                class.non_virtual_methods.push(NonVirtualMethodDeclaration {
                    name: Arc::from(method.name.to_string()),
                    descriptor: Arc::from(method.descriptor.to_string()),
                    access_flags: method.access_flags.bits(),
                });
            }
        }

        // Parse BootstrapMethods attribute (for invokedynamic)
        // Cafebabe already resolved the data — we just convert to our types.
        for attr in &class_file.attributes {
            match &attr.data {
                cafebabe::attributes::AttributeData::BootstrapMethods(entries) => {
                    for entry in entries {
                        let method = Self::convert_method_handle(&entry.method);
                        let arguments = entry
                            .arguments
                            .iter()
                            .map(Self::convert_bootstrap_arg)
                            .collect();
                        class
                            .bootstrap_methods
                            .push(ResolvedBootstrapMethod { method, arguments });
                    }
                }
                cafebabe::attributes::AttributeData::NestHost(host) => {
                    class
                        .nest
                        .get_or_insert_with(|| Box::new(NestMetadata::default()))
                        .host = Some(host.to_string().replace('/', ".").into_boxed_str());
                }
                cafebabe::attributes::AttributeData::NestMembers(members) => {
                    class
                        .nest
                        .get_or_insert_with(|| Box::new(NestMetadata::default()))
                        .members = members
                        .iter()
                        .map(|member| member.to_string().replace('/', ".").into_boxed_str())
                        .collect();
                }
                _ => {}
            }
        }

        // Phase 2: Bytecode verification (JVMS 4.10)
        // Verify each method's bytecode for structural correctness
        for method in &class_file.methods {
            if let Some(code_attr) = method.attributes.iter().find_map(|attr| {
                if let cafebabe::attributes::AttributeData::Code(code) = &attr.data {
                    Some(code)
                } else {
                    None
                }
            }) {
                // Convert cafebabe exception table to our format
                let exception_table: Vec<ExceptionTableEntry> = code_attr
                    .exception_table
                    .iter()
                    .map(|e| ExceptionTableEntry {
                        start_pc: e.start_pc,
                        end_pc: e.end_pc,
                        handler_pc: e.handler_pc,
                        catch_type: e.catch_type.as_ref().map(|s| s.to_string()),
                    })
                    .collect();

                // Verify the bytecode
                verify_bytecode(code_attr.code, &exception_table).map_err(|e| {
                    VMError::VerifyError(format!(
                        "Method {}.{}{}: {}",
                        class.name, method.name, method.descriptor, e
                    ))
                })?;
            }
        }

        Ok(class)
    }

    /// Link a class (resolve superclass, build full vtable)
    pub fn link_class(
        class: &mut LoadedClass,
        class_loader: &super::class_loader::ClassLoader,
    ) -> VMResult<()> {
        // Build the class hierarchy before considering interfaces. Linking is
        // strict: a missing structural dependency is a linkage failure, not a
        // reason to publish an irreparably incomplete class and hope that load
        // order repairs it later.
        let declared_methods = std::mem::take(&mut class.vtable);
        class.resolved_virtual_methods.clear();
        let mut effective_interfaces = class.interfaces.clone();
        if class.is_interface
            && class
                .superclass
                .as_deref()
                .is_some_and(|parent| normalize_class_name(parent) != "java.lang.Object")
        {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "interface {} has invalid superclass {}",
                class.name,
                class.superclass.as_deref().unwrap_or_default()
            )));
        }
        if let Some(ref superclass_name) = class.superclass.clone() {
            // Class files encode Object as an interface's superclass even
            // though interfaces do not inherit Object's virtual method table.
            if !(class.is_interface && normalize_class_name(superclass_name) == "java.lang.Object")
            {
                let superclass = class_loader
                    .get_loaded_class(superclass_name)
                    .map_err(|_| {
                        VMError::NoClassDefFoundError(format!(
                            "{} requires missing superclass {}",
                            class.name, superclass_name
                        ))
                    })?;
                if superclass.is_interface {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "{} cannot extend interface {} as a class",
                        class.name, superclass_name
                    )));
                }
                if superclass.modifiers & cafebabe::ClassAccessFlags::PUBLIC.bits() == 0
                    && !super::method_area::same_runtime_package(&class.name, &superclass.name)
                {
                    return Err(VMError::IllegalAccessError(format!(
                        "{} cannot access superclass {}",
                        class.name, superclass.name
                    )));
                }
                if superclass.modifiers & cafebabe::ClassAccessFlags::FINAL.bits() != 0 {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "{} cannot extend final class {}",
                        class.name, superclass_name
                    )));
                }
                effective_interfaces.extend(superclass.interfaces.clone());
                effective_interfaces.extend(
                    superclass
                        .itable
                        .iter()
                        .map(|route| route.interface_name.to_string()),
                );
                class.field_layout.inherit_from(&superclass.field_layout);
                class.vtable = superclass.vtable;
                class.resolved_virtual_methods = superclass.resolved_virtual_methods;
                class.default_conflict_count = superclass.default_conflict_count;
                // Vtable slots are stable across overrides, so inherited
                // interface routes remain valid and must be visible when the
                // subclass declares no interfaces of its own.
                class.itable = superclass.itable;
            }
        }

        // Every declaration gets its own symbolic route. It also retargets all
        // inherited declaration routes that it can override under JVMS 5.4.5.
        // Keeping both identities is necessary for binary bytecode that lowers
        // visibility: a later subclass can override a public ancestor without
        // overriding the now-inaccessible intermediate declaration.
        for method in declared_methods {
            let mut overridden_declarations = Vec::new();
            let mut overridden_owners = HashSet::new();
            let declared_owner = method.declaring_class().to_string();
            for origin in class
                .vtable
                .iter()
                .filter(|entry| entry.name == method.name && entry.descriptor == method.descriptor)
                .map(|entry| &entry.origin)
            {
                if !origin.directly_overrideable_by(&declared_owner) {
                    continue;
                }
                for overridden in std::iter::once(origin).chain(origin.overridden_declarations()) {
                    if overridden_owners.insert(overridden.declaring_class.to_string()) {
                        overridden_declarations.push(overridden.clone());
                    }
                }
            }

            for overridden in &overridden_declarations {
                if overridden.access_flags & cafebabe::MethodAccessFlags::FINAL.bits() != 0 {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "{} cannot override final method {}.{}{}",
                        class.name, overridden.declaring_class, method.name, method.descriptor
                    )));
                }
            }

            let method = method.with_overridden_declarations(overridden_declarations);
            for route in class.vtable.iter_mut().filter(|entry| {
                entry.name == method.name
                    && entry.descriptor == method.descriptor
                    && method
                        .origin
                        .overridden_declarations()
                        .iter()
                        .any(|overridden| Arc::ptr_eq(overridden, &entry.origin))
            }) {
                route.retarget(method.target.clone());
            }
            class.set_resolved_virtual_method(
                method.name.clone(),
                method.descriptor.clone(),
                method.origin.clone(),
            );
            class.vtable.push(method);
        }

        // Resolve the complete interface closure before selecting defaults.
        // Keeping route names for every superinterface makes an
        // invokeinterface whose symbolic owner is a parent interface resolve on
        // a class that directly implements only its child.
        for interface_name in &class.interfaces {
            let interface = class_loader.get_loaded_class(interface_name).map_err(|_| {
                VMError::NoClassDefFoundError(format!(
                    "{} requires missing interface {}",
                    class.name, interface_name
                ))
            })?;
            if interface.modifiers & cafebabe::ClassAccessFlags::PUBLIC.bits() == 0
                && !super::method_area::same_runtime_package(&class.name, &interface.name)
            {
                return Err(VMError::IllegalAccessError(format!(
                    "{} cannot access superinterface {}",
                    class.name, interface.name
                )));
            }
        }
        let interfaces = interface_closure(class_loader, &class.name, &effective_interfaces)?;
        let mut methods_by_signature: HashMap<
            (Arc<str>, Arc<str>),
            Vec<super::method_area::VTableEntry>,
        > = HashMap::new();
        let mut routes = Vec::new();

        // An interface's own declarations participate alongside declarations
        // from its superinterface closure. They already exist in `class.vtable`
        // because declaration linking precedes default selection.
        if class.is_interface {
            for method in class.vtable.iter().filter(|method| {
                method.origin.declared_in_interface
                    && normalize_class_name(method.dispatch_owner())
                        == normalize_class_name(&class.name)
            }) {
                methods_by_signature
                    .entry((method.name.clone(), method.descriptor.clone()))
                    .or_default()
                    .push(method.clone());
            }
        }

        for (interface_name, interface) in &interfaces {
            if !interface.is_interface {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "{} implements non-interface {}",
                    class.name, interface_name
                )));
            }

            // Candidate declarations are owned by this interface. Inherited
            // selected routes remain in its vtable for symbolic dispatch but
            // are not declarations of the interface itself.
            for method in interface.vtable.iter().filter(|method| {
                normalize_class_name(method.dispatch_owner())
                    == normalize_class_name(interface_name)
            }) {
                let signature = (method.name.clone(), method.descriptor.clone());
                methods_by_signature
                    .entry(signature.clone())
                    .or_default()
                    .push(method.clone());
            }

            // Each symbolic interface owner resolves one declaration per
            // signature. Retain that origin so its itable entry points at the
            // corresponding route rather than a signature-wide shared slot.
            let mut seen_signatures = HashSet::new();
            for method in interface.vtable.iter().rev() {
                let signature = (method.name.clone(), method.descriptor.clone());
                if seen_signatures.insert(signature.clone()) {
                    routes.push((interface_name.clone(), signature, method.origin.clone()));
                }
            }
        }

        for (signature, candidates) in methods_by_signature {
            let class_target = class
                .vtable
                .iter()
                .rfind(|entry| {
                    entry.name == signature.0
                        && entry.descriptor == signature.1
                        && !entry.origin.declared_in_interface
                })
                .map(|entry| entry.target.clone());

            let (selected_target, default_conflict) = if let Some(target) = class_target {
                // A class/superclass declaration, including an abstract or
                // non-public one, takes precedence over interface defaults.
                // invokeinterface enforces the selected method's special
                // public/private runtime constraint at the invocation site.
                (target, false)
            } else {
                let mut declarations = candidates.clone();
                declarations
                    .sort_by(|left, right| left.dispatch_owner().cmp(right.dispatch_owner()));
                declarations
                    .dedup_by(|left, right| left.dispatch_owner() == right.dispatch_owner());

                // First discard declarations shadowed by a more-specific
                // subinterface. This must happen before filtering abstract
                // methods: `Child extends Parent` may intentionally redeclare
                // a Parent default as abstract.
                let maximal: Vec<_> = declarations
                    .iter()
                    .filter(|candidate| {
                        !declarations.iter().any(|other| {
                            candidate.dispatch_owner() != other.dispatch_owner()
                                && ((class.is_interface
                                    && normalize_class_name(other.dispatch_owner())
                                        == normalize_class_name(&class.name))
                                    || interface_extends(
                                        class_loader,
                                        other.dispatch_owner(),
                                        candidate.dispatch_owner(),
                                    ))
                        })
                    })
                    .cloned()
                    .collect();
                let defaults: Vec<_> = maximal
                    .iter()
                    .filter(|candidate| !candidate.is_abstract())
                    .cloned()
                    .collect();
                match defaults.as_slice() {
                    [] => (
                        maximal
                            .first()
                            .expect("an interface signature has a maximally-specific method")
                            .target
                            .clone(),
                        false,
                    ),
                    [only] => (only.target.clone(), false),
                    many => (many[0].target.clone(), true),
                }
            };

            class.set_resolved_virtual_method(
                signature.0.clone(),
                signature.1.clone(),
                if default_conflict {
                    selected_target.with_default_conflict()
                } else {
                    selected_target.clone()
                },
            );

            let candidate_owners: HashSet<_> = candidates
                .iter()
                .map(|candidate| normalize_class_name(candidate.dispatch_owner()))
                .collect();
            for candidate in candidates {
                if !class.vtable.iter().any(|entry| {
                    entry.name == signature.0
                        && entry.descriptor == signature.1
                        && normalize_class_name(entry.dispatch_owner())
                            == normalize_class_name(candidate.dispatch_owner())
                }) {
                    class.vtable.push(candidate);
                }
            }
            for route in class.vtable.iter_mut().filter(|entry| {
                entry.name == signature.0
                    && entry.descriptor == signature.1
                    && entry.origin.declared_in_interface
                    && candidate_owners.contains(&normalize_class_name(entry.dispatch_owner()))
            }) {
                route.retarget(selected_target.clone());
            }
        }

        for (interface_name, signature, symbolic_origin) in routes {
            let slot = class
                .vtable
                .iter()
                .position(|entry| {
                    entry.name == signature.0
                        && entry.descriptor == signature.1
                        && normalize_class_name(entry.dispatch_owner())
                            == normalize_class_name(&symbolic_origin.declaring_class)
                })
                .ok_or_else(|| {
                    VMError::NoSuchMethod(format!(
                        "{}.{}{}",
                        interface_name, signature.0, signature.1
                    ))
                })?;
            if !class.itable.iter().any(|entry| {
                entry.interface_name.as_ref() == interface_name
                    && entry.method_name.as_ref() == signature.0.as_ref()
                    && entry.descriptor.as_ref() == signature.1.as_ref()
            }) {
                class.add_itable_entry(
                    interface_name,
                    signature.0.to_string(),
                    signature.1.to_string(),
                    slot,
                );
            }
        }

        class.refresh_default_conflict_count();
        Ok(())
    }

    /// Convert cafebabe ReferenceKind to our MethodHandleKind
    fn convert_reference_kind(kind: &cafebabe::constant_pool::ReferenceKind) -> MethodHandleKind {
        use cafebabe::constant_pool::ReferenceKind;
        match kind {
            ReferenceKind::GetField => MethodHandleKind::GetField,
            ReferenceKind::GetStatic => MethodHandleKind::GetStatic,
            ReferenceKind::PutField => MethodHandleKind::PutField,
            ReferenceKind::PutStatic => MethodHandleKind::PutStatic,
            ReferenceKind::InvokeVirtual => MethodHandleKind::InvokeVirtual,
            ReferenceKind::InvokeStatic => MethodHandleKind::InvokeStatic,
            ReferenceKind::InvokeSpecial => MethodHandleKind::InvokeSpecial,
            ReferenceKind::NewInvokeSpecial => MethodHandleKind::NewInvokeSpecial,
            ReferenceKind::InvokeInterface => MethodHandleKind::InvokeInterface,
        }
    }

    /// Convert cafebabe MethodHandle to our ResolvedMethodHandle
    fn convert_method_handle(mh: &cafebabe::constant_pool::MethodHandle) -> ResolvedMethodHandle {
        ResolvedMethodHandle {
            kind: Self::convert_reference_kind(&mh.kind),
            class_name: mh.class_name.to_string(),
            member_name: mh.member_ref.name.to_string(),
            descriptor: mh.member_ref.descriptor.to_string(),
        }
    }

    /// Convert cafebabe BootstrapArgument to our BootstrapArg
    fn convert_bootstrap_arg(arg: &cafebabe::constant_pool::BootstrapArgument) -> BootstrapArg {
        use cafebabe::constant_pool::{BootstrapArgument, LiteralConstant};
        match arg {
            BootstrapArgument::LiteralConstant(lit) => match lit {
                LiteralConstant::Integer(v) => BootstrapArg::Int(*v),
                LiteralConstant::Long(v) => BootstrapArg::Long(*v),
                LiteralConstant::Float(v) => BootstrapArg::Float(*v),
                LiteralConstant::Double(v) => BootstrapArg::Double(*v),
                LiteralConstant::String(s) => BootstrapArg::String(s.to_string()),
                LiteralConstant::StringBytes(bytes) => {
                    // Try to convert bytes to UTF-8 string, fall back to lossy conversion
                    BootstrapArg::String(String::from_utf8_lossy(bytes).to_string())
                }
            },
            BootstrapArgument::ClassInfo(name) => BootstrapArg::Class(name.to_string()),
            BootstrapArgument::MethodHandle(mh) => {
                BootstrapArg::MethodHandle(Self::convert_method_handle(mh))
            }
            BootstrapArgument::MethodType(desc) => BootstrapArg::MethodType(desc.to_string()),
        }
    }
}

fn validate_raw_member_metadata(bytecode: &[u8]) -> VMResult<()> {
    fn format_error(message: impl Into<String>) -> VMError {
        VMError::ClassFormatError(message.into())
    }
    fn read_u8(bytes: &[u8], offset: &mut usize) -> VMResult<u8> {
        let value = *bytes
            .get(*offset)
            .ok_or_else(|| VMError::ClassFormatError("truncated class file".into()))?;
        *offset += 1;
        Ok(value)
    }
    fn read_u16(bytes: &[u8], offset: &mut usize) -> VMResult<u16> {
        let data = bytes
            .get(*offset..*offset + 2)
            .ok_or_else(|| VMError::ClassFormatError("truncated class file".into()))?;
        *offset += 2;
        Ok(u16::from_be_bytes([data[0], data[1]]))
    }
    fn read_u32(bytes: &[u8], offset: &mut usize) -> VMResult<u32> {
        let data = bytes
            .get(*offset..*offset + 4)
            .ok_or_else(|| VMError::ClassFormatError("truncated class file".into()))?;
        *offset += 4;
        Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
    }
    fn skip(bytes: &[u8], offset: &mut usize, length: usize) -> VMResult<()> {
        bytes
            .get(*offset..*offset + length)
            .ok_or_else(|| VMError::ClassFormatError("truncated class file".into()))?;
        *offset += length;
        Ok(())
    }
    fn cp_utf8<'a>(utf8: &[Option<&'a [u8]>], index: usize, subject: &str) -> VMResult<&'a [u8]> {
        utf8.get(index)
            .and_then(|value| *value)
            .ok_or_else(|| format_error(format!("invalid UTF-8 index for {subject}")))
    }
    fn valid_unqualified_name(name: &[u8], method: bool) -> bool {
        if name.is_empty()
            || name
                .iter()
                .any(|byte| matches!(byte, b'.' | b';' | b'[' | b'/'))
        {
            return false;
        }
        !method
            || matches!(name, b"<init>" | b"<clinit>")
            || !name.iter().any(|byte| matches!(byte, b'<' | b'>'))
    }
    fn parse_field_type(bytes: &[u8], offset: &mut usize) -> Option<u16> {
        let kind = bytes.get(*offset).copied()?;
        *offset += 1;
        match kind {
            b'J' | b'D' => Some(2),
            b'B' | b'C' | b'F' | b'I' | b'S' | b'Z' => Some(1),
            b'L' => {
                let start = *offset;
                let mut segment_start = start;
                while let Some(byte) = bytes.get(*offset).copied() {
                    *offset += 1;
                    match byte {
                        b';' if *offset - 1 > segment_start => return Some(1),
                        b';' => return None,
                        b'/' if *offset - 1 > segment_start => segment_start = *offset,
                        b'/' | b'.' | b'[' => return None,
                        _ => {}
                    }
                }
                None
            }
            b'[' => {
                let mut dimensions = 1;
                while bytes.get(*offset) == Some(&b'[') {
                    *offset += 1;
                    dimensions += 1;
                    if dimensions > 255 {
                        return None;
                    }
                }
                parse_field_type(bytes, offset).map(|_| 1)
            }
            _ => None,
        }
    }
    fn valid_field_descriptor(bytes: &[u8]) -> bool {
        let mut offset = 0;
        parse_field_type(bytes, &mut offset).is_some() && offset == bytes.len()
    }
    fn valid_method_descriptor(bytes: &[u8], is_static: bool) -> bool {
        if bytes.first() != Some(&b'(') {
            return false;
        }
        let mut offset = 1;
        let mut parameter_slots = u16::from(!is_static);
        while bytes.get(offset) != Some(&b')') {
            let Some(width) = parse_field_type(bytes, &mut offset) else {
                return false;
            };
            let Some(total) = parameter_slots.checked_add(width) else {
                return false;
            };
            parameter_slots = total;
            if parameter_slots > 255 {
                return false;
            }
        }
        offset += 1;
        if bytes.get(offset) == Some(&b'V') {
            offset += 1;
        } else if parse_field_type(bytes, &mut offset).is_none() {
            return false;
        }
        offset == bytes.len()
    }
    fn skip_attributes(bytes: &[u8], offset: &mut usize) -> VMResult<()> {
        let attribute_count = read_u16(bytes, offset)?;
        for _ in 0..attribute_count {
            skip(bytes, offset, 2)?;
            let length = usize::try_from(read_u32(bytes, offset)?)
                .map_err(|_| format_error("member attribute is too large"))?;
            skip(bytes, offset, length)?;
        }
        Ok(())
    }

    if bytecode.get(..4) != Some(&[0xca, 0xfe, 0xba, 0xbe]) {
        return Err(format_error("invalid class-file magic"));
    }
    let mut offset = 8;
    let constant_pool_count = usize::from(read_u16(bytecode, &mut offset)?);
    let mut utf8 = vec![None; constant_pool_count];
    let mut index = 1;
    while index < constant_pool_count {
        let tag = read_u8(bytecode, &mut offset)?;
        match tag {
            1 => {
                let length = usize::from(read_u16(bytecode, &mut offset)?);
                let value = bytecode
                    .get(offset..offset + length)
                    .ok_or_else(|| format_error("truncated constant-pool UTF-8 entry"))?;
                utf8[index] = Some(value);
                offset += length;
            }
            3 | 4 => skip(bytecode, &mut offset, 4)?,
            5 | 6 => {
                skip(bytecode, &mut offset, 8)?;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => skip(bytecode, &mut offset, 2)?,
            9 | 10 | 11 | 12 | 17 | 18 => skip(bytecode, &mut offset, 4)?,
            15 => skip(bytecode, &mut offset, 3)?,
            other => return Err(format_error(format!("invalid constant-pool tag {other}"))),
        }
        index += 1;
    }

    skip(bytecode, &mut offset, 6)?; // access_flags, this_class, super_class
    let interface_count = usize::from(read_u16(bytecode, &mut offset)?);
    skip(bytecode, &mut offset, interface_count * 2)?;
    let field_count = read_u16(bytecode, &mut offset)?;
    for field_index in 0..field_count {
        skip(bytecode, &mut offset, 2)?; // access_flags
        let name_index = usize::from(read_u16(bytecode, &mut offset)?);
        let descriptor_index = usize::from(read_u16(bytecode, &mut offset)?);
        let name = cp_utf8(&utf8, name_index, &format!("field {field_index} name"))?;
        if !valid_unqualified_name(name, false) {
            return Err(format_error(format!(
                "invalid name for field {field_index}"
            )));
        }
        let descriptor = cp_utf8(
            &utf8,
            descriptor_index,
            &format!("field {field_index} descriptor"),
        )?;
        if !valid_field_descriptor(descriptor) {
            return Err(format_error(format!(
                "invalid descriptor for field {field_index}"
            )));
        }
        skip_attributes(bytecode, &mut offset)?;
    }

    let method_count = read_u16(bytecode, &mut offset)?;
    for method_index in 0..method_count {
        let access_flags = read_u16(bytecode, &mut offset)?;
        let name_index = usize::from(read_u16(bytecode, &mut offset)?);
        let descriptor_index = usize::from(read_u16(bytecode, &mut offset)?);
        let name = cp_utf8(&utf8, name_index, &format!("method {method_index} name"))?;
        if !valid_unqualified_name(name, true) {
            return Err(format_error(format!(
                "invalid name for method {method_index}"
            )));
        }
        let descriptor = cp_utf8(
            &utf8,
            descriptor_index,
            &format!("method {method_index} descriptor"),
        )?;
        if !valid_method_descriptor(descriptor, access_flags & 0x0008 != 0)
            || (name == b"<init>" && descriptor.last() != Some(&b'V'))
            || (name == b"<clinit>" && descriptor != b"()V")
        {
            return Err(format_error(format!(
                "invalid descriptor for method {method_index}"
            )));
        }
        skip_attributes(bytecode, &mut offset)?;
    }
    Ok(())
}

fn validate_field_modifiers(
    class_name: &str,
    is_interface: bool,
    field_name: &str,
    descriptor: &str,
    flags: u16,
) -> VMResult<()> {
    const PUBLIC: u16 = 0x0001;
    const PRIVATE: u16 = 0x0002;
    const PROTECTED: u16 = 0x0004;
    const STATIC: u16 = 0x0008;
    const FINAL: u16 = 0x0010;
    const VOLATILE: u16 = 0x0040;
    const TRANSIENT: u16 = 0x0080;
    const ENUM: u16 = 0x4000;

    let visibility = flags & (PUBLIC | PRIVATE | PROTECTED);
    if visibility.count_ones() > 1 {
        return Err(VMError::ClassFormatError(format!(
            "field {class_name}.{field_name}:{descriptor} has conflicting access modifiers"
        )));
    }
    if flags & (FINAL | VOLATILE) == (FINAL | VOLATILE) {
        return Err(VMError::ClassFormatError(format!(
            "field {class_name}.{field_name}:{descriptor} cannot be both final and volatile"
        )));
    }
    if is_interface {
        let required = PUBLIC | STATIC | FINAL;
        let forbidden = PRIVATE | PROTECTED | VOLATILE | TRANSIENT | ENUM;
        if flags & required != required || flags & forbidden != 0 {
            return Err(VMError::ClassFormatError(format!(
                "interface field {class_name}.{field_name}:{descriptor} must be public static final"
            )));
        }
        // ACC_SYNTHETIC is the only optional defined interface-field bit;
        // unknown future bits remain ignored per JVMS 4.5.
    }
    Ok(())
}

fn normalize_class_name(name: &str) -> String {
    name.replace('/', ".")
}

fn interface_closure(
    class_loader: &super::class_loader::ClassLoader,
    owner: &str,
    roots: &[String],
) -> VMResult<Vec<(String, LoadedClass)>> {
    fn visit(
        class_loader: &super::class_loader::ClassLoader,
        owner: &str,
        name: &str,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        result: &mut Vec<(String, LoadedClass)>,
    ) -> VMResult<()> {
        let canonical = normalize_class_name(name);
        if visited.contains(&canonical) {
            return Ok(());
        }
        if !visiting.insert(canonical.clone()) {
            return Err(VMError::ClassFormatError(format!(
                "circular interface hierarchy while linking {owner}: {canonical}"
            )));
        }
        let interface = class_loader.get_loaded_class(&canonical).map_err(|_| {
            VMError::NoClassDefFoundError(format!("{owner} requires missing interface {canonical}"))
        })?;
        if !interface.is_interface {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "{owner} implements non-interface {canonical}"
            )));
        }
        for parent in &interface.interfaces {
            visit(class_loader, owner, parent, visiting, visited, result)?;
        }
        visiting.remove(&canonical);
        visited.insert(canonical.clone());
        result.push((canonical, interface));
        Ok(())
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    for root in roots {
        visit(
            class_loader,
            owner,
            root,
            &mut visiting,
            &mut visited,
            &mut result,
        )?;
    }
    Ok(result)
}

fn interface_extends(
    class_loader: &super::class_loader::ClassLoader,
    candidate: &str,
    ancestor: &str,
) -> bool {
    let ancestor = normalize_class_name(ancestor);
    let mut stack = vec![normalize_class_name(candidate)];
    let mut visited = HashSet::new();
    while let Some(current) = stack.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if current == ancestor {
            return true;
        }
        let Ok(interface) = class_loader.get_loaded_class(&current) else {
            return false;
        };
        stack.extend(
            interface
                .interfaces
                .iter()
                .map(|parent| normalize_class_name(parent)),
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::class_loader::ClassLoader;

    #[test]
    fn test_linker_creation() {
        // Dummy test to verify module compiles
        let _linker = ClassLinker;
    }

    #[test]
    fn field_modifier_constraints_follow_jvms_4_5() {
        assert!(validate_field_modifiers("C", false, "x", "I", 0x0001).is_ok());
        assert!(validate_field_modifiers("C", false, "x", "I", 0x0001 | 0x0002).is_err());
        assert!(validate_field_modifiers("C", false, "x", "I", 0x0010 | 0x0040).is_err());

        let interface_field = 0x0001 | 0x0008 | 0x0010;
        assert!(validate_field_modifiers("I", true, "x", "I", interface_field).is_ok());
        assert!(validate_field_modifiers("I", true, "x", "I", interface_field | 0x1000).is_ok());
        assert!(validate_field_modifiers("I", true, "x", "I", 0x0001 | 0x0008).is_err());
        assert!(validate_field_modifiers("I", true, "x", "I", interface_field | 0x0080).is_err());
    }

    #[test]
    fn constantvalue_kind_must_match_the_field_descriptor() {
        assert!(FieldConstantValue::Int(1).matches_descriptor("Z"));
        assert!(
            FieldConstantValue::String(Arc::from("value")).matches_descriptor("Ljava/lang/String;")
        );
        assert!(!FieldConstantValue::Int(1).matches_descriptor("J"));
        assert!(!FieldConstantValue::String(Arc::from("value"))
            .matches_descriptor("Ljava/lang/Object;"));
    }

    #[test]
    fn missing_superclass_is_a_linkage_failure() {
        let loader = ClassLoader::new();
        let mut child = LoadedClass::new("app.Child".to_string());
        child.set_superclass("dependency.Missing".to_string());

        let error = ClassLinker::link_class(&mut child, &loader).unwrap_err();
        assert!(matches!(error, VMError::NoClassDefFoundError(_)));
        assert!(error.to_string().contains("dependency.Missing"));
    }

    #[test]
    fn subclass_layout_prefixes_declared_fields_with_parent_layout() {
        let loader = ClassLoader::new();
        let mut parent = LoadedClass::new("dependency.Parent".to_string());
        parent.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        parent
            .field_layout
            .add_field_with_descriptor("value".to_string(), "I".to_string());
        loader.register_loaded_class(parent).unwrap();

        let mut child = LoadedClass::new("app.Child".to_string());
        child.set_superclass("dependency.Parent".to_string());
        child
            .field_layout
            .add_field_with_descriptor("value".to_string(), "J".to_string());
        ClassLinker::link_class(&mut child, &loader).unwrap();

        assert_eq!(child.field_layout.field_count, 2);
        assert_eq!(
            child
                .field_layout
                .get_offset_with_descriptor("value", "I")
                .unwrap(),
            0
        );
        assert_eq!(
            child
                .field_layout
                .get_offset_with_descriptor("value", "J")
                .unwrap(),
            1
        );
        assert!(child
            .field_layout
            .get_offset_with_descriptor("value", "D")
            .is_err());
    }

    #[test]
    fn abstract_interface_method_is_not_selected_as_a_default() {
        let loader = ClassLoader::new();
        let mut interface = LoadedClass::new("api.Service".to_string());
        interface.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        interface.is_interface = true;
        interface.add_vtable_entry_with_abstract(
            "required".to_string(),
            "()I".to_string(),
            "api.Service".to_string(),
            true,
        );
        interface.add_vtable_entry_with_abstract(
            "provided".to_string(),
            "()I".to_string(),
            "api.Service".to_string(),
            false,
        );
        loader.register_loaded_class(interface).unwrap();

        let mut implementation = LoadedClass::new("app.Implementation".to_string());
        implementation.add_interface("api.Service".to_string());
        ClassLinker::link_class(&mut implementation, &loader).unwrap();

        let required = implementation
            .vtable
            .iter()
            .find(|method| method.name.as_ref() == "required")
            .unwrap();
        let provided = implementation
            .vtable
            .iter()
            .find(|method| method.name.as_ref() == "provided")
            .unwrap();
        assert!(required.is_abstract());
        assert!(!provided.is_abstract());
        assert_eq!(implementation.itable.len(), 2);
    }

    #[test]
    fn abstract_class_declaration_takes_precedence_over_interface_default() {
        let loader = ClassLoader::new();
        let mut interface = LoadedClass::new("api.DefaultValue".to_string());
        interface.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        interface.is_interface = true;
        interface.add_vtable_entry_with_abstract(
            "value".to_string(),
            "()I".to_string(),
            "api.DefaultValue".to_string(),
            false,
        );
        loader.register_loaded_class(interface).unwrap();

        let mut base = LoadedClass::new("app.AbstractBase".to_string());
        base.add_vtable_entry_with_abstract(
            "value".to_string(),
            "()I".to_string(),
            "app.AbstractBase".to_string(),
            true,
        );
        loader.register_loaded_class(base).unwrap();

        let mut child = LoadedClass::new("app.AbstractChild".to_string());
        child.set_superclass("app.AbstractBase".to_string());
        child.add_interface("api.DefaultValue".to_string());
        ClassLinker::link_class(&mut child, &loader).unwrap();

        let selected = child
            .vtable
            .iter()
            .find(|method| method.name.as_ref() == "value")
            .unwrap();
        assert_eq!(selected.declaring_class(), "app.AbstractBase");
        assert!(selected.is_abstract());
    }

    #[test]
    fn abstract_subinterface_redeclaration_suppresses_parent_default() {
        let loader = ClassLoader::new();
        let mut parent = LoadedClass::new("api.Parent".to_string());
        parent.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        parent.is_interface = true;
        parent.add_vtable_entry_with_abstract(
            "value".to_string(),
            "()I".to_string(),
            "api.Parent".to_string(),
            false,
        );
        loader.register_loaded_class(parent).unwrap();

        let mut child = LoadedClass::new("api.Child".to_string());
        child.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        child.is_interface = true;
        child.add_interface("api.Parent".to_string());
        child.add_vtable_entry_with_abstract(
            "value".to_string(),
            "()I".to_string(),
            "api.Child".to_string(),
            true,
        );
        ClassLinker::link_class(&mut child, &loader).unwrap();
        loader.register_loaded_class(child).unwrap();

        let mut implementation = LoadedClass::new("app.Implementation".to_string());
        implementation.add_interface("api.Child".to_string());
        ClassLinker::link_class(&mut implementation, &loader).unwrap();

        let selected = implementation
            .vtable
            .iter()
            .find(|method| method.name.as_ref() == "value")
            .unwrap();
        assert_eq!(selected.declaring_class(), "api.Child");
        assert!(selected.is_abstract());
    }

    #[test]
    fn circular_interface_hierarchy_is_rejected() {
        let loader = ClassLoader::new();
        let mut left = LoadedClass::new("api.Left".to_string());
        left.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        left.is_interface = true;
        left.add_interface("api.Right".to_string());
        loader.register_loaded_class(left).unwrap();
        let mut right = LoadedClass::new("api.Right".to_string());
        right.modifiers = cafebabe::ClassAccessFlags::PUBLIC.bits();
        right.is_interface = true;
        right.add_interface("api.Left".to_string());
        loader.register_loaded_class(right).unwrap();

        let mut implementation = LoadedClass::new("app.Implementation".to_string());
        implementation.add_interface("api.Left".to_string());
        let error = ClassLinker::link_class(&mut implementation, &loader).unwrap_err();
        assert!(matches!(error, VMError::ClassFormatError(_)));
        assert!(error.to_string().contains("circular interface hierarchy"));
    }
}
