// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constant pool resolution methods for the Executor
//!
//! Handles resolution of method references, field references, invokedynamic,
//! class names, name-and-type entries, UTF8 strings, and bootstrap method refs
//! from the JVM constant pool.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::access_flags;
use crate::interpreter::executor::{Executor, MethodBytecode};
use crate::interpreter::method_area::{FieldDeclaration, MethodArea, StaticFieldKey};
use crate::producer::ConstantPoolItem;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MethodReferenceKind {
    Class,
    Interface,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedField {
    pub symbolic_owner: Arc<str>,
    pub declaring_class: Arc<str>,
    pub declaration: FieldDeclaration,
    pub instance_offset: Option<usize>,
    pub is_native_stub: bool,
}

impl ResolvedField {
    pub(crate) fn static_key(&self) -> StaticFieldKey {
        (
            Arc::clone(&self.declaring_class),
            Arc::clone(&self.declaration.name),
            Arc::clone(&self.declaration.descriptor),
        )
    }
}

impl Executor {
    #[cfg(test)]
    pub(crate) fn extract_package(class_name: &str) -> &str {
        class_name
            .rfind(['.', '/'])
            .map_or("", |separator| &class_name[..separator])
    }
    /// Resolve a method reference from the constant pool
    ///
    /// Given a constant pool index pointing to a CONSTANT_Methodref or CONSTANT_InterfaceMethodref,
    /// returns (class_name, method_name, descriptor)
    pub fn resolve_method_ref(&self, cp_index: usize) -> VMResult<(String, String, String)> {
        self.resolve_method_ref_with_kind(cp_index)
            .map(|(class, name, descriptor, _)| (class, name, descriptor))
    }

    pub(crate) fn resolve_class_method_ref(
        &self,
        cp_index: usize,
    ) -> VMResult<(String, String, String)> {
        match self.resolve_method_ref_with_kind(cp_index)? {
            (class, name, descriptor, MethodReferenceKind::Class) => Ok((class, name, descriptor)),
            _ => Err(VMError::InvalidBytecode),
        }
    }

    pub(crate) fn resolve_interface_method_ref(
        &self,
        cp_index: usize,
    ) -> VMResult<(String, String, String)> {
        match self.resolve_method_ref_with_kind(cp_index)? {
            (class, name, descriptor, MethodReferenceKind::Interface) => {
                Ok((class, name, descriptor))
            }
            _ => Err(VMError::InvalidBytecode),
        }
    }

    pub(crate) fn resolve_method_ref_with_kind(
        &self,
        cp_index: usize,
    ) -> VMResult<(String, String, String, MethodReferenceKind)> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        // This is defensive programming against malformed bytecode
        let (class_index, name_and_type_index, kind) = match cp.get(cp_index) {
            Some(ConstantPoolItem::MethodRef {
                class_index,
                name_and_type_index,
            }) => (
                *class_index as usize,
                *name_and_type_index as usize,
                MethodReferenceKind::Class,
            ),
            Some(ConstantPoolItem::InterfaceMethodRef {
                class_index,
                name_and_type_index,
            }) => (
                *class_index as usize,
                *name_and_type_index as usize,
                MethodReferenceKind::Interface,
            ),
            _ => return Err(VMError::InvalidBytecode),
        };

        // Resolve class name from Class constant pool entry
        let class_name = self.resolve_class_name(class_index)?;

        // Resolve method name and descriptor from NameAndType entry
        let (method_name, descriptor) = self.resolve_name_and_type(name_and_type_index)?;

        Ok((class_name, method_name, descriptor, kind))
    }

    /// Resolve a method by searching the class hierarchy (for invokespecial)
    ///
    /// Per JVMS §6.5.invokespecial: When a method is not found in the referenced
    /// class, we must search up the superclass chain. This happens when:
    /// - super.method() references Parent.method() but method is in GrandParent
    ///
    /// Returns (actual_class_name, method_bytecode) where actual_class_name is
    /// the class where the method was actually found.
    /// Look up a method in the registry under both the given class-name form and
    /// its slash/dot counterpart (the registry is keyed inconsistently across
    /// entry points). Returns the matched class name and bytecode.
    pub(crate) fn lookup_method_either_form(
        &self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
    ) -> Option<(String, MethodBytecode)> {
        let primary = (
            class_name.to_string(),
            method_name.to_string(),
            descriptor.to_string(),
        );
        if let Some(m) = self.method_registry.get(&primary) {
            return Some((class_name.to_string(), m.clone()));
        }
        let alt = if class_name.contains('/') {
            class_name.replace('/', ".")
        } else {
            class_name.replace('.', "/")
        };
        if alt != class_name {
            let key = (alt.clone(), method_name.to_string(), descriptor.to_string());
            if let Some(m) = self.method_registry.get(&key) {
                return Some((alt, m.clone()));
            }
        }
        None
    }

    pub(crate) fn resolve_method_in_hierarchy(
        &self,
        start_class: &str,
        method_name: &str,
        descriptor: &str,
    ) -> VMResult<(String, MethodBytecode)> {
        // Different entry points key the method registry by different class-name
        // forms (the main VM path uses the declared dot form; the Gradle plugin
        // path uses the internal slash form), and constant-pool refs arrive in
        // slash form. Try the name as given and its slash/dot counterpart.
        let mut current_class = start_class.to_string();

        loop {
            // Try to find method in current class (either name form).
            if let Some((found_class, method)) =
                self.lookup_method_either_form(&current_class, method_name, descriptor)
            {
                return Ok((found_class, method));
            }

            // Method not found - try superclass
            let superclass = if let Some(method_area) = &self.method_area {
                let method_area_locked = method_area.read().map_err(|_| {
                    VMError::InternalError("Failed to acquire read lock on method_area".to_string())
                })?;
                match method_area_locked.get_class(&current_class) {
                    Ok(class) => class.superclass.clone(),
                    Err(_) => None, // Class not loaded, can't continue
                }
            } else {
                None
            };

            match superclass {
                Some(parent) if !parent.is_empty() => {
                    current_class = parent.replace('/', ".");
                }
                _ => {
                    // No more superclasses to search
                    return Err(VMError::NoSuchMethod(format!(
                        "{}.{}{}",
                        start_class, method_name, descriptor
                    )));
                }
            }
        }
    }

    pub(crate) fn class_implements_interface(
        &self,
        class_name: &str,
        interface_name: &str,
    ) -> bool {
        let Some(method_area) = &self.method_area else {
            return false;
        };
        let Ok(method_area) = method_area.read() else {
            return false;
        };
        let wanted = interface_name.replace('/', ".");
        let mut stack = vec![class_name.replace('/', ".")];
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            let Ok(class) = method_area.get_class(&current) else {
                continue;
            };
            for interface in &class.interfaces {
                let interface = interface.replace('/', ".");
                if interface == wanted {
                    return true;
                }
                stack.push(interface);
            }
            if let Some(superclass) = class.superclass {
                stack.push(superclass.replace('/', "."));
            }
        }
        false
    }

    fn same_runtime_package(left: &str, right: &str) -> bool {
        crate::interpreter::method_area::same_runtime_package(left, right)
    }

    fn effective_nest_host(&self, class_name: &str) -> String {
        let Some(method_area) = &self.method_area else {
            return class_name.replace('/', ".");
        };
        let Ok(method_area) = method_area.read() else {
            return class_name.replace('/', ".");
        };
        let Ok(class) = method_area.get_class(class_name) else {
            return class_name.replace('/', ".");
        };
        let Some(host_name) = class.nest.as_deref().and_then(|nest| nest.host.as_deref()) else {
            return class.name.replace('/', ".");
        };
        let Ok(host) = method_area.get_class(host_name) else {
            return class.name.replace('/', ".");
        };
        let canonical_class = class.name.replace('/', ".");
        if Self::same_runtime_package(&canonical_class, &host.name)
            && host
                .nest
                .as_deref()
                .into_iter()
                .flat_map(|nest| nest.members.iter())
                .any(|member| member.replace('/', ".") == canonical_class)
        {
            host.name.replace('/', ".")
        } else {
            canonical_class
        }
    }

    fn same_nest(&self, left: &str, right: &str) -> bool {
        self.effective_nest_host(left) == self.effective_nest_host(right)
    }

    /// Check protected access per JVMS 5.4.4
    /// Protected members are accessible from:
    /// 1. Same class
    /// 2. Same package
    /// 3. Subclasses (with restrictions on instance access)
    pub(crate) fn check_protected_access(
        &self,
        caller_class: &str,
        target_class: &str,
        declaring_class: &str,
    ) -> bool {
        // Same class: always OK
        if caller_class.replace('/', ".") == declaring_class.replace('/', ".") {
            return true;
        }

        // Same package: OK
        if Self::same_runtime_package(caller_class, declaring_class) {
            return true;
        }

        // Subclass check: caller must be subclass of declaring class
        // AND target must be caller or subclass of caller
        let caller = caller_class.replace('/', ".");
        let target = target_class.replace('/', ".");
        let declaring = declaring_class.replace('/', ".");
        self.is_subclass_of(&caller, &declaring)
            && (target == caller || self.is_subclass_of(&target, &caller))
    }

    /// Check method access control per JVMS 5.4.4
    ///
    /// Verifies that the caller has permission to invoke the target method.
    /// - Private methods: Only accessible from the same class
    /// - Protected methods: Accessible from same package or subclasses
    /// - Package-private (no modifier): Accessible from same package
    /// - Public methods: Always accessible
    pub(crate) fn check_method_access(
        &self,
        caller_class: &str,
        declaring_class: &str,
        method_name: &str,
        access_flags_val: u16,
    ) -> VMResult<()> {
        self.check_method_access_on_receiver(
            caller_class,
            declaring_class,
            declaring_class,
            method_name,
            access_flags_val,
        )
    }

    pub(crate) fn check_method_access_on_receiver(
        &self,
        caller_class: &str,
        declaring_class: &str,
        receiver_class: &str,
        method_name: &str,
        access_flags_val: u16,
    ) -> VMResult<()> {
        // Public: always accessible
        if access_flags_val & access_flags::ACC_PUBLIC != 0 {
            return Ok(());
        }

        // Private: only same class
        if access_flags_val & access_flags::ACC_PRIVATE != 0 {
            if !self.same_nest(caller_class, declaring_class) {
                return Err(VMError::IllegalAccessError(format!(
                    "Cannot access private method {}.{} from {}",
                    declaring_class, method_name, caller_class
                )));
            }
            return Ok(());
        }

        // Protected: same package or subclass with restrictions
        if access_flags_val & access_flags::ACC_PROTECTED != 0 {
            if self.check_protected_access(caller_class, receiver_class, declaring_class) {
                return Ok(());
            }

            return Err(VMError::IllegalAccessError(format!(
                "Cannot access protected method {}.{} from {}",
                declaring_class, method_name, caller_class
            )));
        }

        // Package-private (no access modifier): only same package
        if Self::same_runtime_package(caller_class, declaring_class) {
            return Ok(());
        }

        Err(VMError::IllegalAccessError(format!(
            "Cannot access package-private method {}.{} from {}",
            declaring_class, method_name, caller_class
        )))
    }

    /// Check symbolic member accessibility before an instance receiver is
    /// examined. The protected receiver-qualifier constraint is checked later
    /// once the runtime receiver class is known.
    pub(crate) fn check_method_symbolic_access(
        &self,
        caller_class: &str,
        declaring_class: &str,
        method_name: &str,
        access_flags_val: u16,
    ) -> VMResult<()> {
        if access_flags_val & access_flags::ACC_PUBLIC != 0 {
            return Ok(());
        }
        if access_flags_val & access_flags::ACC_PRIVATE != 0 {
            if self.same_nest(caller_class, declaring_class) {
                return Ok(());
            }
        } else if access_flags_val & access_flags::ACC_PROTECTED != 0 {
            let caller = caller_class.replace('/', ".");
            let declaring = declaring_class.replace('/', ".");
            if Self::same_runtime_package(&caller, &declaring)
                || self.is_subclass_of(&caller, &declaring)
            {
                return Ok(());
            }
        } else if Self::same_runtime_package(caller_class, declaring_class) {
            return Ok(());
        }
        Err(VMError::IllegalAccessError(format!(
            "Cannot access method {}.{} from {}",
            declaring_class, method_name, caller_class
        )))
    }

    pub(crate) fn check_symbolic_class_access(
        &self,
        caller_class: &str,
        symbolic_owner: &str,
    ) -> VMResult<()> {
        if symbolic_owner.starts_with('[') {
            let component = symbolic_owner.trim_start_matches('[');
            if component.len() == 1 && b"ZBCSIJFD".contains(&component.as_bytes()[0]) {
                return Ok(());
            }
            let reference_component = component
                .strip_prefix('L')
                .and_then(|name| name.strip_suffix(';'))
                .ok_or(VMError::InvalidBytecode)?;
            return self.check_symbolic_class_access(caller_class, reference_component);
        }
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;
        let class = method_area.get_class(symbolic_owner)?;
        if class.modifiers & access_flags::ACC_PUBLIC != 0
            || Self::same_runtime_package(caller_class, &class.name)
        {
            return Ok(());
        }
        Err(VMError::IllegalAccessError(format!(
            "Cannot access symbolic class {} from {}",
            class.name, caller_class
        )))
    }

    /// Resolve a field reference from the constant pool
    ///
    /// Given a constant pool index pointing to a CONSTANT_Fieldref,
    /// returns (class_name, field_name, descriptor)
    pub fn resolve_field_ref(&self, cp_index: usize) -> VMResult<(String, String, String)> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        let (class_index, name_and_type_index) = match cp.get(cp_index) {
            Some(ConstantPoolItem::FieldRef {
                class_index,
                name_and_type_index,
            }) => (*class_index as usize, *name_and_type_index as usize),
            _ => return Err(VMError::InvalidBytecode),
        };

        // Resolve class name from Class constant pool entry
        let class_name = self.resolve_class_name(class_index)?;

        // Resolve field name and descriptor from NameAndType entry
        let (field_name, descriptor) = self.resolve_name_and_type(name_and_type_index)?;

        Ok((class_name, field_name, descriptor))
    }

    /// Resolve a symbolic field reference according to JVMS 5.4.3.2. The
    /// declaration search order is the symbolic class itself, its direct
    /// superinterfaces recursively, and finally its superclass.
    pub(crate) fn resolve_field(
        &self,
        cp_index: usize,
        caller_class: &str,
    ) -> VMResult<ResolvedField> {
        let (symbolic_owner, field_name, descriptor) = self.resolve_field_ref(cp_index)?;
        self.check_symbolic_class_access(caller_class, &symbolic_owner)
            .map_err(|error| field_resolution_error(error, &symbolic_owner))?;
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire MethodArea lock"))?;
        let mut visited = std::collections::HashSet::new();
        let mut resolved = resolve_field_in_hierarchy(
            &method_area,
            &symbolic_owner,
            &field_name,
            &descriptor,
            &mut visited,
        )
        .map_err(|error| field_resolution_error(error, &symbolic_owner))?
        .ok_or_else(|| {
            VMError::NoSuchField(format!("{symbolic_owner}.{field_name}:{descriptor}"))
        })?;
        drop(method_area);
        resolved.symbolic_owner = Arc::from(symbolic_owner);
        self.check_field_symbolic_access(caller_class, &resolved)?;
        Ok(resolved)
    }

    fn check_field_symbolic_access(
        &self,
        caller_class: &str,
        resolved: &ResolvedField,
    ) -> VMResult<()> {
        let flags = resolved.declaration.access_flags;
        if flags & access_flags::ACC_PUBLIC != 0 {
            return Ok(());
        }
        if flags & access_flags::ACC_PRIVATE != 0 {
            if self.same_nest(caller_class, &resolved.declaring_class) {
                return Ok(());
            }
        } else if flags & access_flags::ACC_PROTECTED != 0 {
            let same_package = Self::same_runtime_package(caller_class, &resolved.declaring_class);
            let caller_is_subclass = self.is_subclass_of(caller_class, &resolved.declaring_class);
            let static_field = flags & access_flags::ACC_STATIC != 0;
            let owner_is_caller_or_subclass = caller_class.replace('/', ".")
                == resolved.symbolic_owner.replace('/', ".")
                || self.is_subclass_of(&resolved.symbolic_owner, caller_class);
            // JVMS 5.4.4: for a protected instance member accessed from a
            // subclass in another package, the class named by the symbolic
            // reference must be the caller or one of its subclasses. This is
            // a resolution-time rule and therefore precedes a null check.
            if same_package || (caller_is_subclass && (static_field || owner_is_caller_or_subclass))
            {
                return Ok(());
            }
        } else if Self::same_runtime_package(caller_class, &resolved.declaring_class) {
            return Ok(());
        }
        Err(VMError::IllegalAccessError(format!(
            "Cannot access field {}.{}:{} from {}",
            resolved.declaring_class,
            resolved.declaration.name,
            resolved.declaration.descriptor,
            caller_class
        )))
    }

    /// Resolve an invokedynamic reference from the constant pool
    ///
    /// Given a constant pool index pointing to a CONSTANT_InvokeDynamic,
    /// returns (bootstrap_method_attr_index, call_site_name, method_type_descriptor)
    pub fn resolve_invokedynamic(&self, cp_index: usize) -> VMResult<(u16, String, String)> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        let (bootstrap_method_attr_index, name_and_type_index) = match cp.get(cp_index) {
            Some(ConstantPoolItem::InvokeDynamic {
                bootstrap_method_attr_index,
                name_and_type_index,
            }) => (*bootstrap_method_attr_index, *name_and_type_index as usize),
            _ => return Err(VMError::InvalidBytecode),
        };

        // Resolve call site name and method type descriptor from NameAndType entry
        let (call_site_name, method_type_descriptor) =
            self.resolve_name_and_type(name_and_type_index)?;

        Ok((
            bootstrap_method_attr_index,
            call_site_name,
            method_type_descriptor,
        ))
    }

    /// Helper: Resolve class name from a Class constant pool entry
    pub(crate) fn resolve_class_name(&self, cp_index: usize) -> VMResult<String> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        let name_index = match cp.get(cp_index) {
            Some(ConstantPoolItem::Class { name_index }) => *name_index as usize,
            _ => return Err(VMError::InvalidBytecode),
        };

        self.resolve_utf8(name_index)
    }

    /// Read a u8 operand from bytecode and advance pc
    #[inline]
    pub(crate) fn read_u8_operand(&mut self) -> VMResult<u8> {
        if self.pc >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let value = self.bytecode[self.pc];
        self.pc += 1;
        Ok(value)
    }

    /// Read a u16 operand from bytecode and advance pc
    #[inline]
    pub(crate) fn read_u16_operand(&mut self) -> VMResult<u16> {
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let value = u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]);
        self.pc += 2;
        Ok(value)
    }

    /// Helper: Resolve name and descriptor from a NameAndType constant pool entry
    pub(crate) fn resolve_name_and_type(&self, cp_index: usize) -> VMResult<(String, String)> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        let (name_index, descriptor_index) = match cp.get(cp_index) {
            Some(ConstantPoolItem::NameAndType {
                name_index,
                descriptor_index,
            }) => (*name_index as usize, *descriptor_index as usize),
            _ => return Err(VMError::InvalidBytecode),
        };

        let name = self.resolve_utf8(name_index)?;
        let descriptor = self.resolve_utf8(descriptor_index)?;
        Ok((name, descriptor))
    }

    /// Helper: Resolve a UTF8 string from the constant pool
    pub(crate) fn resolve_utf8(&self, cp_index: usize) -> VMResult<String> {
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or(VMError::InvalidBytecode)?;

        // H10_2 FIX: Use .get() instead of direct indexing to prevent panic on out-of-bounds
        match cp.get(cp_index) {
            Some(ConstantPoolItem::Utf8(s)) => Ok(s.clone()),
            _ => Err(VMError::InvalidBytecode),
        }
    }
}

fn resolve_field_in_hierarchy(
    method_area: &MethodArea,
    owner: &str,
    field_name: &str,
    descriptor: &str,
    visited: &mut std::collections::HashSet<String>,
) -> VMResult<Option<ResolvedField>> {
    let mut pending = vec![owner.to_string()];
    while let Some(owner) = pending.pop() {
        let class = method_area.get_class(&owner)?;
        let canonical_owner = class.name.replace('/', ".");
        if !visited.insert(canonical_owner) {
            continue;
        }

        if let Some(declaration) = class.declared_fields.iter().find(|field| {
            field.name.as_ref() == field_name && field.descriptor.as_ref() == descriptor
        }) {
            let instance_offset = (declaration.access_flags & access_flags::ACC_STATIC == 0)
                .then(|| {
                    class
                        .field_layout
                        .get_offset_with_descriptor(field_name, descriptor)
                })
                .transpose()?;
            return Ok(Some(ResolvedField {
                symbolic_owner: Arc::from(""),
                declaring_class: Arc::from(class.name.replace('/', ".")),
                declaration: declaration.clone(),
                instance_offset,
                is_native_stub: class.is_native_stub,
            }));
        }

        // LIFO order reproduces JVMS resolution: direct superinterfaces and
        // their closures are visited before the superclass.
        if !class.is_interface {
            if let Some(superclass) = class.superclass {
                pending.push(superclass);
            }
        }
        pending.extend(class.interfaces.into_iter().rev());
    }
    Ok(None)
}

/// A missing class discovered while resolving a symbolic Fieldref is a JVM
/// linkage failure, not the VM's internal class-lookup signal. Preserve every
/// other error (including lock/infrastructure failures) verbatim.
fn field_resolution_error(error: VMError, symbolic_owner: &str) -> VMError {
    match error {
        VMError::ClassNotFound(missing) => VMError::NoClassDefFoundError(format!(
            "{symbolic_owner} field resolution requires {missing}"
        )),
        other => other,
    }
}

#[cfg(test)]
mod field_resolution_tests {
    use super::{field_resolution_error, resolve_field_in_hierarchy};
    use crate::interpreter::executor::access_flags;
    use crate::interpreter::method_area::{FieldDeclaration, LoadedClass};
    use crate::interpreter::{MethodArea, VMError};
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn missing_field_resolution_supertypes_become_noclassdeffounderror() {
        for use_interface in [false, true] {
            let method_area = MethodArea::new();
            let mut owner = LoadedClass::new("field.Owner".to_string());
            if use_interface {
                owner.add_interface("missing.Interface".to_string());
            } else {
                owner.set_superclass("missing.Parent".to_string());
            }
            method_area.register_class(owner).unwrap();

            let error = resolve_field_in_hierarchy(
                &method_area,
                "field.Owner",
                "value",
                "I",
                &mut HashSet::new(),
            )
            .unwrap_err();
            let error = field_resolution_error(error, "field.Owner");
            assert!(matches!(error, VMError::NoClassDefFoundError(_)));
            assert!(error.is_catchable());
        }
    }

    #[test]
    fn interface_field_resolution_never_searches_classfile_object_super() {
        let method_area = MethodArea::new();
        let mut object = LoadedClass::new("java.lang.Object".to_string());
        object.declared_fields = Arc::from([FieldDeclaration::new(
            "value",
            "I",
            access_flags::ACC_PUBLIC | access_flags::ACC_STATIC,
        )]);
        method_area.register_class(object).unwrap();

        let mut sibling = LoadedClass::new("field.Sibling".to_string());
        sibling.is_interface = true;
        sibling.set_superclass("java.lang.Object".to_string());
        sibling.declared_fields = Arc::from([FieldDeclaration::new(
            "value",
            "I",
            access_flags::ACC_PUBLIC | access_flags::ACC_STATIC,
        )]);
        method_area.register_class(sibling).unwrap();

        let mut owner = LoadedClass::new("field.Owner".to_string());
        owner.is_interface = true;
        // Class files encode Object here, but it is not an interface
        // superclass for JVMS field-resolution purposes.
        owner.set_superclass("java.lang.Object".to_string());
        owner.add_interface("field.Sibling".to_string());
        method_area.register_class(owner).unwrap();

        let resolved = resolve_field_in_hierarchy(
            &method_area,
            "field.Owner",
            "value",
            "I",
            &mut HashSet::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.declaring_class.as_ref(), "field.Sibling");
    }
}
