// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM method area and class metadata storage

use super::bootstrap::ResolvedBootstrapMethod;
use super::errors::{VMError, VMResult};
use super::heap::Heap;
use super::monitor::ThreadId;
use super::value::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub(crate) type StaticFieldKey = (Arc<str>, Arc<str>, Arc<str>);

/// One field declaration as it appears in a class file. Field identity includes
/// the descriptor: malformed or generated class files may legally declare two
/// fields with the same name and different descriptors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldDeclaration {
    pub name: Arc<str>,
    pub descriptor: Arc<str>,
    pub access_flags: u16,
}

impl FieldDeclaration {
    pub fn new(
        name: impl Into<Arc<str>>,
        descriptor: impl Into<Arc<str>>,
        access_flags: u16,
    ) -> Self {
        Self {
            name: name.into(),
            descriptor: descriptor.into(),
            access_flags,
        }
    }
}

/// A JVM ConstantValue attribute retained until the interpreter has a heap in
/// which String constants can be interned.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldConstantValue {
    Int(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(Arc<str>),
}

impl FieldConstantValue {
    pub(crate) fn matches_descriptor(&self, descriptor: &str) -> bool {
        matches!(
            (self, descriptor),
            (Self::Int(_), "B" | "C" | "I" | "S" | "Z")
                | (Self::Float(_), "F")
                | (Self::Long(_), "J")
                | (Self::Double(_), "D")
                | (Self::String(_), "Ljava/lang/String;")
        )
    }

    fn prepared_primitive(&self, descriptor: &str) -> Option<Value> {
        match self {
            Self::Int(value) => Some(Value::Int(match descriptor {
                "B" => i32::from(*value as i8),
                "C" => i32::from(*value as u16),
                "S" => i32::from(*value as i16),
                "Z" => i32::from(*value != 0),
                _ => *value,
            })),
            Self::Float(value) => Some(Value::Float(*value)),
            Self::Long(value) => Some(Value::Long(*value)),
            Self::Double(value) => Some(Value::Double(*value)),
            Self::String(_) => None,
        }
    }
}

/// Sparse ConstantValue metadata, indexed by the declaration's position in
/// [`LoadedClass::declared_fields`]. Most fields do not carry this attribute.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstantFieldValue {
    pub declaration_index: u16,
    pub value: FieldConstantValue,
}

/// Field layout information (offsets for instance fields)
#[derive(Clone, Debug)]
pub struct FieldLayout {
    /// Field name and descriptor -> offset mapping.
    ///
    /// Java source normally prevents two fields with the same name in one
    /// class, but the class-file identity of a field includes its descriptor.
    /// Keeping that identity here also lets a subclass hide a parent field
    /// without reusing the parent's object slot.
    field_offsets_by_descriptor: HashMap<(String, String), usize>,
    /// Total number of instance fields
    pub field_count: usize,
}

impl FieldLayout {
    /// Create a new field layout
    pub fn new() -> Self {
        FieldLayout {
            field_offsets_by_descriptor: HashMap::new(),
            field_count: 0,
        }
    }

    /// Add a field at the next available offset
    pub fn add_field(&mut self, name: String) -> usize {
        self.add_field_with_descriptor(name, String::new())
    }

    /// Add a declared field while retaining its class-file descriptor.
    pub fn add_field_with_descriptor(&mut self, name: String, descriptor: String) -> usize {
        let offset = self.field_count;
        self.field_offsets_by_descriptor
            .insert((name, descriptor), offset);
        self.field_count += 1;
        offset
    }

    /// Prefix this class's declared fields with an already-linked superclass
    /// layout. Parent offsets remain stable, while hidden child fields receive
    /// fresh slots after every inherited field.
    pub fn inherit_from(&mut self, parent: &Self) {
        let mut declared: Vec<_> = std::mem::take(&mut self.field_offsets_by_descriptor)
            .into_iter()
            .collect();
        declared.sort_by_key(|(_, offset)| *offset);
        *self = parent.clone();
        for ((name, descriptor), _) in declared {
            self.add_field_with_descriptor(name, descriptor);
        }
    }

    /// Get field offset by name
    pub fn get_offset(&self, name: &str) -> VMResult<usize> {
        self.field_offsets_by_descriptor
            .iter()
            .filter_map(|((candidate, _), offset)| (candidate == name).then_some(*offset))
            .max()
            .ok_or_else(|| VMError::NoSuchField(name.to_string()))
    }

    /// Resolve a field by its class-file identity, retaining the name-only
    /// fallback for synthetic/stub layouts created by older VM APIs.
    pub fn get_offset_with_descriptor(&self, name: &str, descriptor: &str) -> VMResult<usize> {
        self.field_offsets_by_descriptor
            .get(&(name.to_string(), descriptor.to_string()))
            .copied()
            .or_else(|| {
                self.field_offsets_by_descriptor
                    .get(&(name.to_string(), String::new()))
                    .copied()
            })
            .ok_or_else(|| VMError::NoSuchField(format!("{name}:{descriptor}")))
    }
}

impl Default for FieldLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// Class initialization state
/// Per JVM Spec §12.4.2, tracks which thread is performing initialization
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassInitState {
    /// Class not yet initialized
    NotInitialized,
    /// Class initialization in progress by the specified thread
    /// Per JVM Spec §12.4.2, only this thread should proceed;
    /// other threads must wait for initialization to complete
    Initializing(ThreadId),
    /// Class fully initialized
    Initialized,
    /// Class initialization failed with the specified error
    /// Per JVM Spec §12.4.1, subsequent access should throw NoClassDefFoundError
    InitError(String),
}

/// One class-file method declaration. Vtable routes share these immutable
/// records so inherited tables do not clone owner names per slot.
#[derive(Debug)]
pub struct VirtualMethodDeclaration {
    /// Class or interface that declares the method.
    pub declaring_class: Box<str>,
    /// Exact class-file access flags for this declaration.
    pub access_flags: u16,
    /// Whether this declaration originates in an interface. Interface-selected
    /// routes can be inherited in a class vtable, but they must not masquerade
    /// as class declarations when a subclass performs default selection.
    pub declared_in_interface: bool,
    /// Method resolution succeeded, but runtime selection has unrelated
    /// maximally-specific defaults and must throw ICCE when invoked.
    pub default_conflict: bool,
    /// Transitive declaration identities this method can override under JVMS
    /// 5.4.5. Keeping the graph on declarations makes package-access bridges
    /// explicit and lets two symbolic routes diverge again in a subclass.
    overridden_declarations: Vec<Arc<VirtualMethodDeclaration>>,
}

impl VirtualMethodDeclaration {
    fn new(declaring_class: String, access_flags: u16, declared_in_interface: bool) -> Arc<Self> {
        Arc::new(Self {
            declaring_class: declaring_class.into_boxed_str(),
            access_flags,
            declared_in_interface,
            default_conflict: false,
            overridden_declarations: Vec::new(),
        })
    }

    pub(crate) fn directly_overrideable_by(&self, subclass: &str) -> bool {
        if self.access_flags & 0x0001 != 0 || self.access_flags & 0x0004 != 0 {
            return true;
        }
        // Private declarations never enter vtables. An origin with none of
        // the public/protected/private bits is package-private.
        self.access_flags & 0x0002 == 0 && same_runtime_package(&self.declaring_class, subclass)
    }

    pub(crate) fn overridden_declarations(&self) -> &[Arc<VirtualMethodDeclaration>] {
        &self.overridden_declarations
    }

    pub(crate) fn with_default_conflict(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self {
            declaring_class: self.declaring_class.clone(),
            access_flags: self.access_flags,
            declared_in_interface: self.declared_in_interface,
            default_conflict: true,
            overridden_declarations: self.overridden_declarations.clone(),
        })
    }
}

/// One symbolic virtual-dispatch route. Every declaration owns a distinct
/// route (`origin`); overrides update its selected `target` without destroying
/// that identity.
#[derive(Clone, Debug)]
pub struct VTableEntry {
    /// Method name
    pub name: Arc<str>,
    /// Method descriptor (e.g., "(II)I" for int method(int, int))
    pub descriptor: Arc<str>,
    /// Method currently selected when dispatch starts from this route.
    pub target: Arc<VirtualMethodDeclaration>,
    /// Declaration identity resolved by the symbolic method reference.
    pub origin: Arc<VirtualMethodDeclaration>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedVirtualMethod {
    name: Arc<str>,
    descriptor: Arc<str>,
    declaration: Arc<VirtualMethodDeclaration>,
}

impl VTableEntry {
    pub(crate) fn new(
        name: String,
        descriptor: String,
        declaring_class: String,
        access_flags: u16,
        declared_in_interface: bool,
    ) -> Self {
        let declaration =
            VirtualMethodDeclaration::new(declaring_class, access_flags, declared_in_interface);
        Self {
            name: Arc::from(name),
            descriptor: Arc::from(descriptor),
            target: declaration.clone(),
            origin: declaration,
        }
    }

    /// Reject dispatch through metadata that deliberately has no method body.
    pub fn require_concrete(&self, receiver_class: &str) -> VMResult<()> {
        if self.is_abstract() {
            Err(VMError::AbstractMethodError(format!(
                "{receiver_class}.{}{}",
                self.name, self.descriptor
            )))
        } else {
            Ok(())
        }
    }

    pub fn is_final(&self) -> bool {
        self.target.access_flags & 0x0010 != 0
    }

    pub fn is_abstract(&self) -> bool {
        self.target.access_flags & 0x0400 != 0
    }

    pub fn declaring_class(&self) -> &str {
        &self.target.declaring_class
    }

    pub fn access_flags(&self) -> u16 {
        self.target.access_flags
    }

    pub fn dispatch_owner(&self) -> &str {
        &self.origin.declaring_class
    }

    pub(crate) fn with_overridden_declarations(
        self,
        overridden_declarations: Vec<Arc<VirtualMethodDeclaration>>,
    ) -> Self {
        let declaration = Arc::new(VirtualMethodDeclaration {
            declaring_class: self.target.declaring_class.clone(),
            access_flags: self.target.access_flags,
            declared_in_interface: self.target.declared_in_interface,
            default_conflict: self.target.default_conflict,
            overridden_declarations,
        });
        Self {
            name: self.name,
            descriptor: self.descriptor,
            target: declaration.clone(),
            origin: declaration,
        }
    }

    pub(crate) fn retarget(&mut self, target: Arc<VirtualMethodDeclaration>) {
        self.target = target;
    }
}

fn runtime_package(class_name: &str) -> &str {
    class_name
        .rfind(['.', '/'])
        .map_or("", |separator| &class_name[..separator])
}

pub(crate) fn same_runtime_package(left: &str, right: &str) -> bool {
    runtime_package(left)
        .bytes()
        .map(|byte| if byte == b'/' { b'.' } else { byte })
        .eq(runtime_package(right)
            .bytes()
            .map(|byte| if byte == b'/' { b'.' } else { byte }))
}

/// Interface method table entry
#[derive(Clone, Debug)]
pub struct ITableEntry {
    /// Interface name
    pub interface_name: Arc<str>,
    /// Method name
    pub method_name: Arc<str>,
    /// Method descriptor
    pub descriptor: Arc<str>,
    /// VTable index for the implementing method
    pub vtable_index: usize,
}

/// A declared method intentionally excluded from virtual/default candidates.
#[derive(Clone, Debug)]
pub struct NonVirtualMethodDeclaration {
    pub name: Arc<str>,
    pub descriptor: Arc<str>,
    pub access_flags: u16,
}

/// Optional nestmate metadata. Ordinary top-level classes pay only one pointer.
#[derive(Clone, Debug, Default)]
pub struct NestMetadata {
    pub host: Option<Box<str>>,
    pub members: Box<[Box<str>]>,
}

/// Loaded and linked class metadata
#[derive(Clone, Debug)]
pub struct LoadedClass {
    /// Fully qualified class name
    pub name: String,

    /// Superclass name (None for Object)
    pub superclass: Option<String>,

    /// Implemented interfaces
    pub interfaces: Vec<String>,

    /// Virtual method table (for invokevirtual)
    pub vtable: Vec<VTableEntry>,

    /// Explicit method-resolution result per virtual signature. Physical route
    /// order cannot encode this once interface routes coexist with class routes.
    /// Sorted symbolic resolution table. Arc-backed signatures make inherited
    /// snapshots cheap, while binary search avoids allocating lookup keys.
    pub(crate) resolved_virtual_methods: Vec<ResolvedVirtualMethod>,

    /// Static/private declarations excluded from vtables.
    pub non_virtual_methods: Vec<NonVirtualMethodDeclaration>,

    /// Interface method table (for invokeinterface)
    pub itable: Vec<ITableEntry>,

    /// Instance field layout
    pub field_layout: FieldLayout,

    /// Initialization state
    pub init_state: ClassInitState,

    /// Fields declared directly by this class. Inherited fields are resolved by
    /// walking the hierarchy instead of being copied into every subclass.
    pub declared_fields: Arc<[FieldDeclaration]>,

    /// Sparse ConstantValue attributes keyed by declaration index.
    pub constant_field_values: Arc<[ConstantFieldValue]>,

    /// Class modifiers (public, final, abstract, etc.)
    pub modifiers: u16,

    /// Whether this is an interface
    pub is_interface: bool,

    /// True only for metadata materialized by the built-in native stub registry.
    pub is_native_stub: bool,

    /// Fast negative for the extraordinarily rare default-conflict path.
    pub default_conflict_count: u16,

    /// Source file name (if available)
    pub source_file: Option<String>,

    /// NestHost/NestMembers attributes, absent for ordinary top-level classes.
    pub nest: Option<Box<NestMetadata>>,

    /// Bootstrap methods (for invokedynamic instructions)
    /// Stored as resolved data from cafebabe parsing — no CP re-resolution needed.
    pub bootstrap_methods: Vec<ResolvedBootstrapMethod>,
}

impl LoadedClass {
    /// Create a new loaded class
    pub fn new(name: String) -> Self {
        LoadedClass {
            name,
            superclass: None,
            interfaces: Vec::new(),
            vtable: Vec::new(),
            resolved_virtual_methods: Vec::new(),
            non_virtual_methods: Vec::new(),
            itable: Vec::new(),
            field_layout: FieldLayout::new(),
            init_state: ClassInitState::NotInitialized,
            declared_fields: Arc::from([]),
            constant_field_values: Arc::from([]),
            modifiers: 0,
            is_interface: false,
            is_native_stub: false,
            default_conflict_count: 0,
            source_file: None,
            nest: None,
            bootstrap_methods: Vec::new(),
        }
    }

    /// Set the superclass
    pub fn set_superclass(&mut self, superclass: String) {
        self.superclass = Some(superclass);
    }

    /// Add an interface
    pub fn add_interface(&mut self, interface: String) {
        self.interfaces.push(interface);
    }

    /// Add a virtual method
    pub fn add_vtable_entry(&mut self, name: String, descriptor: String, declaring_class: String) {
        self.add_vtable_entry_with_flags(name, descriptor, declaring_class, 0x0001);
    }

    pub fn add_vtable_entry_with_abstract(
        &mut self,
        name: String,
        descriptor: String,
        declaring_class: String,
        is_abstract: bool,
    ) {
        let access_flags = 0x0001 | if is_abstract { 0x0400 } else { 0 };
        self.add_vtable_entry_with_flags(name, descriptor, declaring_class, access_flags);
    }

    pub fn add_vtable_entry_with_flags(
        &mut self,
        name: String,
        descriptor: String,
        declaring_class: String,
        access_flags: u16,
    ) {
        let entry = VTableEntry::new(
            name,
            descriptor,
            declaring_class,
            access_flags,
            self.is_interface,
        );
        self.set_resolved_virtual_method(
            entry.name.clone(),
            entry.descriptor.clone(),
            entry.origin.clone(),
        );
        self.vtable.push(entry);
    }

    /// Add an interface method
    pub fn add_itable_entry(
        &mut self,
        interface_name: String,
        method_name: String,
        descriptor: String,
        vtable_index: usize,
    ) {
        self.itable.push(ITableEntry {
            interface_name: Arc::from(interface_name),
            method_name: Arc::from(method_name),
            descriptor: Arc::from(descriptor),
            vtable_index,
        });
    }

    /// Get VTable index for a method
    pub fn get_vtable_index(&self, name: &str, descriptor: &str) -> VMResult<usize> {
        self.vtable
            .iter()
            .rposition(|e| e.name.as_ref() == name && e.descriptor.as_ref() == descriptor)
            .ok_or_else(|| VMError::NoSuchMethod(format!("{}({})", name, descriptor)))
    }

    /// Resolve a symbolic class's most-specific declaration for invokevirtual.
    pub fn resolve_virtual_declaration(
        &self,
        name: &str,
        descriptor: &str,
    ) -> VMResult<Arc<VirtualMethodDeclaration>> {
        self.resolved_virtual_methods
            .binary_search_by(|entry| {
                (entry.name.as_ref(), entry.descriptor.as_ref()).cmp(&(name, descriptor))
            })
            .ok()
            .map(|index| self.resolved_virtual_methods[index].declaration.clone())
            .ok_or_else(|| VMError::NoSuchMethod(format!("{}.{}{}", self.name, name, descriptor)))
    }

    pub(crate) fn set_resolved_virtual_method(
        &mut self,
        name: Arc<str>,
        descriptor: Arc<str>,
        declaration: Arc<VirtualMethodDeclaration>,
    ) {
        match self.resolved_virtual_methods.binary_search_by(|entry| {
            (entry.name.as_ref(), entry.descriptor.as_ref())
                .cmp(&(name.as_ref(), descriptor.as_ref()))
        }) {
            Ok(index) => self.resolved_virtual_methods[index].declaration = declaration,
            Err(index) => self.resolved_virtual_methods.insert(
                index,
                ResolvedVirtualMethod {
                    name,
                    descriptor,
                    declaration,
                },
            ),
        }
    }

    pub(crate) fn refresh_default_conflict_count(&mut self) {
        self.default_conflict_count = self
            .resolved_virtual_methods
            .iter()
            .filter(|resolution| resolution.declaration.default_conflict)
            .count()
            .try_into()
            .unwrap_or(u16::MAX);
    }

    pub fn declared_non_virtual_method(
        &self,
        name: &str,
        descriptor: &str,
    ) -> Option<&NonVirtualMethodDeclaration> {
        self.non_virtual_methods
            .iter()
            .find(|method| method.name.as_ref() == name && method.descriptor.as_ref() == descriptor)
    }

    pub fn has_default_method_conflict(&self, name: &str, descriptor: &str) -> bool {
        self.default_conflict_count != 0
            && self
                .resolve_virtual_declaration(name, descriptor)
                .is_ok_and(|resolution| resolution.default_conflict)
    }

    /// Select the runtime target for an already-resolved dispatch slot.
    pub fn dispatch_target(
        &self,
        name: &str,
        descriptor: &str,
        origin: &Arc<VirtualMethodDeclaration>,
    ) -> VMResult<&VTableEntry> {
        self.vtable
            .iter()
            .find(|entry| {
                entry.name.as_ref() == name
                    && entry.descriptor.as_ref() == descriptor
                    // Staging and authoritative MethodAreas may materialize
                    // the same JDK stub independently. This VM currently has
                    // one defining-loader namespace, so declaration identity
                    // is the owner plus the already-matched signature rather
                    // than Arc allocation identity.
                    && entry.origin.declaring_class == origin.declaring_class
            })
            .ok_or_else(|| {
                VMError::NoSuchMethod(format!(
                    "{}.{}{} (dispatch slot {})",
                    self.name, name, descriptor, origin.declaring_class
                ))
            })
    }
}

/// Method area for loaded class metadata
pub struct MethodArea {
    /// Loaded classes (name -> LoadedClass)
    loaded_classes: Arc<RwLock<HashMap<String, LoadedClass>>>,

    /// Static field values, keyed by complete class-file field identity.
    static_fields: Arc<RwLock<HashMap<StaticFieldKey, Value>>>,

    /// JDK stub class registry for built-in JDK class support
    stub_registry: super::jdk_stubs::JdkStubRegistry,
}

impl MethodArea {
    fn registry_key(name: &str) -> Cow<'_, str> {
        if name.as_bytes().contains(&b'/') {
            Cow::Owned(name.replace('/', "."))
        } else {
            Cow::Borrowed(name)
        }
    }
    /// Create a new method area
    pub fn new() -> Self {
        MethodArea {
            loaded_classes: Arc::new(RwLock::new(HashMap::new())),
            static_fields: Arc::new(RwLock::new(HashMap::new())),
            stub_registry: super::jdk_stubs::JdkStubRegistry::new(),
        }
    }

    /// Register a loaded class and initialize its static fields with default values
    ///
    /// Per JVM Spec §2.3, static fields are initialized to default values:
    /// - Numeric types (byte, short, int, long, float, double) → 0
    /// - boolean → false (represented as 0)
    /// - char → '\u0000' (represented as 0)
    /// - Reference types → null
    ///
    /// The <clinit> method (if present) will later update these to their declared values.
    pub fn register_class(&self, class: LoadedClass) -> VMResult<()> {
        self.register_classes(vec![class])
    }

    /// Atomically register a set of already-linked classes.
    ///
    /// Batch definition rejects redefinition and acquires both metadata locks
    /// before changing either map. Class readers therefore see the complete
    /// metadata batch or none of it, with static defaults installed before the
    /// metadata becomes visible. A late lock/redefinition failure cannot strand
    /// a child without the parent linked in the same transaction.
    pub fn register_classes(&self, new_classes: Vec<LoadedClass>) -> VMResult<()> {
        // Validate the complete batch and build its static-slot image before
        // taking either publication lock. From this point onward the commit
        // contains no fallible per-class work and cannot leave partial residue.
        let mut names = std::collections::HashSet::new();
        let mut staged_static_fields = HashMap::new();
        for class in &new_classes {
            let class_key = Self::registry_key(&class.name).into_owned();
            if !names.insert(class_key.clone()) {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "duplicate definition of {}",
                    class.name
                )));
            }
            let owner: Arc<str> = Arc::from(class_key);
            let mut declarations = std::collections::HashSet::new();
            for field in class.declared_fields.iter() {
                if !declarations.insert((field.name.as_ref(), field.descriptor.as_ref())) {
                    return Err(VMError::ClassFormatError(format!(
                        "duplicate field {}.{}:{}",
                        class.name, field.name, field.descriptor
                    )));
                }
                if field.access_flags & 0x0008 != 0 {
                    staged_static_fields.insert(
                        (
                            Arc::clone(&owner),
                            Arc::clone(&field.name),
                            Arc::clone(&field.descriptor),
                        ),
                        Self::default_value_for_descriptor(&field.descriptor),
                    );
                }
            }

            let mut constant_targets = std::collections::HashSet::new();
            for constant in class.constant_field_values.iter() {
                if !constant_targets.insert(constant.declaration_index) {
                    return Err(VMError::ClassFormatError(format!(
                        "duplicate ConstantValue metadata in {}",
                        class.name
                    )));
                }
                let declaration = class
                    .declared_fields
                    .get(usize::from(constant.declaration_index))
                    .ok_or_else(|| {
                        VMError::ClassFormatError(format!(
                            "invalid ConstantValue field in {}",
                            class.name
                        ))
                    })?;
                if !constant.value.matches_descriptor(&declaration.descriptor) {
                    return Err(VMError::ClassFormatError(format!(
                        "ConstantValue type does not match {}.{}:{}",
                        class.name, declaration.name, declaration.descriptor
                    )));
                }
                // Class-file ConstantValue on an instance field is ignored by
                // preparation. Native metadata follows the same rule.
                if class.is_native_stub && declaration.access_flags & 0x0008 != 0 {
                    if let Some(value) = constant.value.prepared_primitive(&declaration.descriptor)
                    {
                        staged_static_fields.insert(
                            (
                                Arc::clone(&owner),
                                Arc::clone(&declaration.name),
                                Arc::clone(&declaration.descriptor),
                            ),
                            value,
                        );
                    }
                }
            }
        }

        let mut classes = self
            .loaded_classes
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let mut static_fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        for class in &new_classes {
            let key = Self::registry_key(&class.name).into_owned();
            if classes.contains_key(&key) {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "duplicate definition of {}",
                    class.name
                )));
            }
        }
        static_fields.extend(staged_static_fields);
        for class in new_classes {
            classes.insert(Self::registry_key(&class.name).into_owned(), class);
        }
        Ok(())
    }

    /// Get the default value for a field based on its type descriptor
    ///
    /// Per JVM Spec §2.3:
    /// - Numeric types → 0 (Int, Long, Float, Double)
    /// - boolean → false (Int 0)
    /// - char → '\u0000' (Int 0)
    /// - Reference types → null
    pub(crate) fn default_value_for_descriptor(descriptor: &str) -> Value {
        match descriptor.chars().next() {
            Some('B') => Value::Int(0),      // byte
            Some('C') => Value::Int(0),      // char
            Some('D') => Value::Double(0.0), // double
            Some('F') => Value::Float(0.0),  // float
            Some('I') => Value::Int(0),      // int
            Some('J') => Value::Long(0),     // long
            Some('S') => Value::Int(0),      // short
            Some('Z') => Value::Int(0),      // boolean (false)
            Some('L') | Some('[') => Value::Reference(super::value::ObjectRef::Null), // reference or array
            _ => Value::Reference(super::value::ObjectRef::Null), // Unknown type, default to null
        }
    }

    /// Get a loaded class
    ///
    /// First checks explicitly loaded classes, then falls back to JDK stubs.
    /// JDK stubs and their structural dependencies are linked before they are
    /// atomically published in the same registry as parsed classes.
    pub fn get_class(&self, name: &str) -> VMResult<LoadedClass> {
        self.materialize_stub(name, &mut std::collections::HashSet::new())
    }

    fn materialize_stub(
        &self,
        name: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> VMResult<LoadedClass> {
        match self.get_defined_class(name) {
            Ok(class) => return Ok(class),
            Err(VMError::ClassNotFound(_)) => {}
            Err(error) => return Err(error),
        }

        let canonical_name = Self::registry_key(name).into_owned();
        if !visiting.insert(canonical_name.clone()) {
            return Err(VMError::ClassFormatError(format!(
                "circular native stub hierarchy while linking {canonical_name}"
            )));
        }

        let result = (|| {
            let stub_name = canonical_name.replace('.', "/");
            let stub = self
                .stub_registry
                .get_class(&stub_name)
                .cloned()
                .ok_or_else(|| VMError::ClassNotFound(name.to_string()))?;

            if let Some(superclass) = &stub.superclass {
                self.materialize_stub_dependency(&canonical_name, superclass, visiting)?;
            }
            for interface in &stub.interfaces {
                self.materialize_stub_dependency(&canonical_name, interface, visiting)?;
            }

            let mut loaded_class = stub.to_loaded_class();
            let loader = super::class_loader::ClassLoader::with_method_area(self.clone());
            super::linker::ClassLinker::link_class(&mut loaded_class, &loader)?;

            match self.register_class(loaded_class.clone()) {
                Ok(()) => Ok(loaded_class),
                Err(error) => {
                    // Linking happens without publication locks, so another
                    // thread (or a user definition) may win this race. Return
                    // that single authoritative definition rather than a
                    // private duplicate of the native metadata.
                    match self.get_defined_class(&canonical_name) {
                        Ok(existing) => Ok(existing),
                        Err(VMError::ClassNotFound(_)) => Err(error),
                        Err(lock_error) => Err(lock_error),
                    }
                }
            }
        })();

        visiting.remove(&canonical_name);
        result
    }

    fn materialize_stub_dependency(
        &self,
        owner: &str,
        dependency: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> VMResult<()> {
        self.materialize_stub(dependency, visiting)
            .map(|_| ())
            .map_err(|error| match error {
                VMError::ClassNotFound(_) => VMError::NoClassDefFoundError(format!(
                    "native stub {owner} requires missing structural dependency {dependency}"
                )),
                other => other,
            })
    }

    /// Test named class/interface assignability under one steady-state metadata
    /// lock. Missing native stubs are materialized between read transactions;
    /// ordinary loaded graphs require only the single traversal lock.
    pub(crate) fn is_subtype(&self, source: &str, target: &str) -> VMResult<bool> {
        let source = Self::registry_key(source).into_owned();
        let target = Self::registry_key(target).into_owned();
        loop {
            let classes = self
                .loaded_classes
                .read()
                .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
            let mut missing = Vec::new();
            for required in [&source, &target] {
                if !classes.contains_key(required) {
                    missing.push(required.clone());
                }
            }
            if missing.is_empty() {
                let mut pending = vec![source.clone()];
                let mut visited = std::collections::HashSet::new();
                while let Some(current) = pending.pop() {
                    let current = Self::registry_key(&current).into_owned();
                    if !visited.insert(current.clone()) {
                        continue;
                    }
                    if current == target {
                        return Ok(true);
                    }
                    let Some(class) = classes.get(&current) else {
                        missing.push(current);
                        continue;
                    };
                    if let Some(superclass) = &class.superclass {
                        pending.push(superclass.clone());
                    }
                    pending.extend(class.interfaces.iter().cloned());
                }
                if missing.is_empty() {
                    return Ok(false);
                }
            }
            drop(classes);

            missing.sort_unstable();
            missing.dedup();
            for name in missing {
                match self.get_class(&name) {
                    Ok(_) => {}
                    Err(VMError::ClassNotFound(_)) => return Ok(false),
                    Err(error) => return Err(error),
                }
            }
        }
    }

    /// Read only explicitly published metadata. Unlike `get_class`, this never
    /// materializes a native stub and is therefore safe for staging lookups.
    pub(crate) fn get_defined_class(&self, name: &str) -> VMResult<LoadedClass> {
        self.loaded_classes
            .read()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?
            .get(Self::registry_key(name).as_ref())
            .cloned()
            .ok_or_else(|| VMError::ClassNotFound(name.to_string()))
    }

    /// Check if a class is available (either loaded or in stub registry)
    pub fn class_available(&self, name: &str) -> bool {
        self.class_exists(name) || self.stub_registry.is_stub_class(&name.replace('.', "/"))
    }

    /// Get access to the stub registry for direct queries
    pub fn stub_registry(&self) -> &super::jdk_stubs::JdkStubRegistry {
        &self.stub_registry
    }

    /// Update class initialization state
    pub fn set_init_state(&self, class_name: &str, state: ClassInitState) -> VMResult<()> {
        let mut classes = self
            .loaded_classes
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        if let Some(class) = classes.get_mut(Self::registry_key(class_name).as_ref()) {
            class.init_state = state;
            Ok(())
        } else {
            Err(VMError::ClassNotFound(class_name.to_string()))
        }
    }

    /// Mark a class erroneous only when `owner` still owns its in-progress
    /// initialization. This keeps a late infrastructure error from overwriting
    /// an already-completed state or a more specific recursive failure.
    pub(crate) fn fail_initialization_if_owned(
        &self,
        class_name: &str,
        owner: ThreadId,
        message: String,
    ) -> VMResult<()> {
        let mut classes = self
            .loaded_classes
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let class = classes
            .get_mut(Self::registry_key(class_name).as_ref())
            .ok_or_else(|| VMError::ClassNotFound(class_name.to_string()))?;
        if class.init_state == ClassInitState::Initializing(owner) {
            class.init_state = ClassInitState::InitError(message);
        }
        Ok(())
    }

    /// Check if class exists
    pub fn class_exists(&self, name: &str) -> bool {
        self.loaded_classes
            .read()
            .map(|classes| classes.contains_key(Self::registry_key(name).as_ref()))
            .unwrap_or(false)
    }

    /// Get a snapshot of all loaded classes (for MOP resolution)
    pub fn all_classes(&self) -> HashMap<String, LoadedClass> {
        // A poisoned lock still holds the classes; a panic elsewhere must not hide them.
        self.loaded_classes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Exact bytecode lookup using the already-resolved, Arc-backed identity.
    /// Cloning this key only bumps reference counts and performs no heap
    /// allocation on the opcode hot path.
    pub(crate) fn get_static_resolved(&self, key: &StaticFieldKey) -> VMResult<Value> {
        let fields = self
            .static_fields
            .read()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        fields
            .get(key)
            .cloned()
            .ok_or_else(|| VMError::NoSuchField(format!("{}.{}:{}", key.0, key.1, key.2)))
    }

    /// Exact bytecode store using the already-resolved, Arc-backed identity.
    pub(crate) fn put_static_resolved(&self, key: &StaticFieldKey, value: Value) -> VMResult<()> {
        let mut fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let slot = fields
            .get_mut(key)
            .ok_or_else(|| VMError::NoSuchField(format!("{}.{}:{}", key.0, key.1, key.2)))?;
        *slot = value;
        Ok(())
    }

    /// Descriptor-aware compatibility lookup for callers that have not gone
    /// through symbolic bytecode resolution. This may allocate its lookup key.
    pub fn get_static_exact(
        &self,
        class_name: &str,
        field_name: &str,
        descriptor: &str,
    ) -> VMResult<Value> {
        self.get_static_resolved(&(
            Arc::from(Self::registry_key(class_name).into_owned()),
            Arc::from(field_name),
            Arc::from(descriptor),
        ))
    }

    /// Descriptor-aware compatibility store for tests and runtime helpers.
    pub fn put_static_exact(
        &self,
        class_name: &str,
        field_name: &str,
        descriptor: &str,
        value: Value,
    ) -> VMResult<()> {
        self.put_static_resolved(
            &(
                Arc::from(Self::registry_key(class_name).into_owned()),
                Arc::from(field_name),
                Arc::from(descriptor),
            ),
            value,
        )
    }

    /// Publish a lazily allocated native-stub singleton only if its slot still
    /// contains the prepared null default, and return the authoritative slot.
    /// A racing allocation may become unreachable in the caller's heap; the VM
    /// currently executes a heap through one mutable owner, so this is only a
    /// defensive MethodArea invariant rather than an expected hot path.
    pub(crate) fn publish_static_reference_if_null(
        &self,
        key: &StaticFieldKey,
        candidate: Value,
    ) -> VMResult<Value> {
        let mut fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let slot = fields
            .get_mut(key)
            .ok_or_else(|| VMError::NoSuchField(format!("{}.{}:{}", key.0, key.1, key.2)))?;
        if matches!(slot, Value::Reference(super::value::ObjectRef::Null)) {
            *slot = candidate;
        }
        Ok(slot.clone())
    }

    /// Publish one canonical reference across a group of aliased native static
    /// fields under a single lock (for example `Color.RED` and `Color.red`).
    pub(crate) fn publish_static_alias_group_if_null(
        &self,
        keys: &[StaticFieldKey],
        candidate: Value,
    ) -> VMResult<Value> {
        let mut fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::internal("Failed to acquire static-field lock"))?;
        for key in keys {
            if !fields.contains_key(key) {
                return Err(VMError::NoSuchField(format!(
                    "{}.{}:{}",
                    key.0, key.1, key.2
                )));
            }
        }
        let canonical = keys
            .iter()
            .filter_map(|key| fields.get(key))
            .find(|value| !matches!(value, Value::Reference(super::value::ObjectRef::Null)))
            .cloned()
            .unwrap_or(candidate);
        for key in keys {
            *fields.get_mut(key).expect("validated alias key") = canonical.clone();
        }
        Ok(canonical)
    }

    /// Assign ConstantValue attributes during the preparation portion of class
    /// initialization. This happens once, before superclass initialization and
    /// before the class's own `<clinit>`, so legal initializer writes update the
    /// same canonical slots subsequently read by `getstatic`.
    pub(crate) fn prepare_constant_fields(
        &self,
        class_name: &str,
        heap: &mut Heap,
    ) -> VMResult<()> {
        let class = self.get_class(class_name)?;
        let class_key = Self::registry_key(&class.name).into_owned();
        if heap.constants_prepared(&class_key) {
            return Ok(());
        }
        let owner: Arc<str> = Arc::from(Self::registry_key(&class.name).into_owned());
        let mut prepared = Vec::with_capacity(class.constant_field_values.len());
        for constant in class.constant_field_values.iter() {
            let declaration = class
                .declared_fields
                .get(usize::from(constant.declaration_index))
                .ok_or_else(|| VMError::ClassFormatError("invalid ConstantValue field".into()))?;
            if declaration.access_flags & 0x0008 == 0 {
                continue;
            }
            let value = constant
                .value
                .prepared_primitive(&declaration.descriptor)
                .map(Ok)
                .unwrap_or_else(|| match &constant.value {
                    FieldConstantValue::String(value) => {
                        Ok(Value::Reference(heap.intern_string(value)?))
                    }
                    _ => Err(VMError::ClassFormatError(
                        "invalid ConstantValue metadata".into(),
                    )),
                })?;
            prepared.push((
                (
                    Arc::clone(&owner),
                    Arc::clone(&declaration.name),
                    Arc::clone(&declaration.descriptor),
                ),
                value,
            ));
        }
        {
            let mut fields = self
                .static_fields
                .write()
                .map_err(|_| VMError::internal("static-field lock poisoned"))?;
            for (key, value) in prepared {
                let slot = fields.get_mut(&key).ok_or_else(|| {
                    VMError::NoSuchField(format!("{}.{}:{}", key.0, key.1, key.2))
                })?;
                *slot = value;
            }
        }
        heap.mark_constants_prepared(&class_key);
        Ok(())
    }

    /// Compatibility lookup for runtime helpers whose descriptor is not
    /// available. Bytecode instructions must use [`Self::get_static_exact`].
    pub fn get_static(&self, class_name: &str, field_name: &str) -> VMResult<Value> {
        let class_key = Self::registry_key(class_name);
        let fields = self
            .static_fields
            .read()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let mut matches = fields.iter().filter(|((owner, name, _), _)| {
            owner.as_ref() == class_key.as_ref() && name.as_ref() == field_name
        });
        let value = matches
            .next()
            .map(|(_, value)| value.clone())
            .ok_or_else(|| VMError::NoSuchField(format!("{}.{}", class_name, field_name)))?;
        if matches.next().is_some() {
            return Err(VMError::NoSuchField(format!(
                "ambiguous descriptor for {}.{}",
                class_name, field_name
            )));
        }
        Ok(value)
    }

    /// Compatibility store for tests and runtime helpers. It refuses ambiguous
    /// or absent names instead of silently creating a field.
    pub fn put_static(&self, class_name: &str, field_name: &str, value: Value) -> VMResult<()> {
        let class_key = Self::registry_key(class_name);
        let mut fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let keys: Vec<_> = fields
            .keys()
            .filter(|(owner, name, _)| {
                owner.as_ref() == class_key.as_ref() && name.as_ref() == field_name
            })
            .cloned()
            .collect();
        match keys.as_slice() {
            [key] => {
                *fields.get_mut(key).expect("key came from the same map") = value;
                Ok(())
            }
            [] => Err(VMError::NoSuchField(format!(
                "{}.{}",
                class_name, field_name
            ))),
            _ => Err(VMError::NoSuchField(format!(
                "ambiguous descriptor for {}.{}",
                class_name, field_name
            ))),
        }
    }

    /// Initialize static fields for a class (called during class initialization)
    pub fn init_static_fields(
        &self,
        class_name: &str,
        fields: &[(String, String, Value)],
    ) -> VMResult<()> {
        let mut static_fields = self
            .static_fields
            .write()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        for (field_name, descriptor, value) in fields {
            let key: StaticFieldKey = (
                Arc::from(Self::registry_key(class_name).into_owned()),
                Arc::from(field_name.as_str()),
                Arc::from(descriptor.as_str()),
            );
            let slot = static_fields.get_mut(&key).ok_or_else(|| {
                VMError::NoSuchField(format!("{}.{}:{}", class_name, field_name, descriptor))
            })?;
            *slot = value.clone();
        }
        Ok(())
    }

    /// Get all static field values (for GC root collection)
    pub fn static_field_values(&self) -> Vec<Value> {
        // A poisoned lock still holds the fields; a panic elsewhere must not hide them.
        self.static_fields
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }
}

impl Default for MethodArea {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MethodArea {
    fn clone(&self) -> Self {
        MethodArea {
            loaded_classes: Arc::clone(&self.loaded_classes),
            static_fields: Arc::clone(&self.static_fields),
            stub_registry: self.stub_registry.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::jdk_stubs::{StubClass, StubMethodDescriptor};

    #[test]
    fn test_field_layout() {
        let mut layout = FieldLayout::new();
        assert_eq!(layout.add_field("name".to_string()), 0);
        assert_eq!(layout.add_field("age".to_string()), 1);
        assert_eq!(layout.get_offset("name").unwrap(), 0);
        assert_eq!(layout.get_offset("age").unwrap(), 1);
        assert!(layout.get_offset("missing").is_err());
    }

    #[test]
    fn descriptor_lookup_only_falls_back_to_legacy_descriptorless_fields() {
        let mut layout = FieldLayout::new();
        layout.add_field_with_descriptor("typed".to_string(), "I".to_string());
        layout.add_field("legacy".to_string());

        assert!(layout.get_offset_with_descriptor("typed", "J").is_err());
        assert_eq!(
            layout
                .get_offset_with_descriptor("legacy", "Ljava/lang/Object;")
                .unwrap(),
            1
        );
    }

    #[test]
    fn atomic_registration_does_not_partially_publish_on_redefinition() {
        let method_area = MethodArea::new();
        method_area
            .register_class(LoadedClass::new("existing.Type".to_string()))
            .unwrap();

        let error = method_area
            .register_classes(vec![
                LoadedClass::new("new.Valid".to_string()),
                LoadedClass::new("existing.Type".to_string()),
            ])
            .unwrap_err();
        assert!(matches!(error, VMError::IncompatibleClassChangeError(_)));
        assert!(!method_area.class_exists("new.Valid"));
    }

    #[test]
    fn malformed_native_constant_metadata_does_not_partially_publish() {
        let method_area = MethodArea::new();
        let mut valid = LoadedClass::new("new.Valid".to_string());
        valid.declared_fields = Arc::from([FieldDeclaration::new("VALUE", "I", 0x0009)]);
        let mut malformed = LoadedClass::new("new.Malformed".to_string());
        malformed.is_native_stub = true;
        malformed.constant_field_values = Arc::from([ConstantFieldValue {
            declaration_index: 7,
            value: FieldConstantValue::Int(42),
        }]);

        let error = method_area
            .register_classes(vec![valid, malformed])
            .unwrap_err();
        assert!(matches!(error, VMError::ClassFormatError(_)));
        assert!(!method_area.class_exists("new.Valid"));
        assert!(!method_area.class_exists("new.Malformed"));
        assert!(
            method_area
                .get_static_exact("new.Valid", "VALUE", "I")
                .is_err(),
            "failed batch left a static slot behind"
        );
    }

    #[test]
    fn abstract_vtable_entry_fails_dispatch_explicitly() {
        let entry = VTableEntry::new(
            "required".to_string(),
            "()V".to_string(),
            "api.Service".to_string(),
            0x0401,
            true,
        );
        let error = entry.require_concrete("app.Implementation").unwrap_err();
        assert!(matches!(error, VMError::AbstractMethodError(_)));
        assert_eq!(
            error.to_string(),
            "Abstract method error: app.Implementation.required()V"
        );
    }

    #[test]
    fn vtable_entry_stays_compact() {
        assert!(std::mem::size_of::<VTableEntry>() <= 48);
        assert!(std::mem::size_of::<ITableEntry>() <= 56);
        assert!(std::mem::size_of::<NonVirtualMethodDeclaration>() <= 40);
        assert!(std::mem::size_of::<FieldLayout>() <= 56);
        assert!(std::mem::size_of::<FieldDeclaration>() <= 40);
        assert!(std::mem::size_of::<ConstantFieldValue>() <= 32);
        assert!(std::mem::size_of::<StaticFieldKey>() <= 48);
        assert_eq!(
            std::mem::size_of::<Option<Box<NestMetadata>>>(),
            std::mem::size_of::<usize>()
        );
        let loaded_class_size = std::mem::size_of::<LoadedClass>();
        assert!(
            loaded_class_size <= 360,
            "LoadedClass is {loaded_class_size} bytes"
        );
    }

    #[test]
    fn native_stub_provenance_does_not_follow_a_reserved_name() {
        let method_area = MethodArea::new();
        let user_definition = LoadedClass::new("java.lang.Integer".to_string());
        method_area.register_class(user_definition).unwrap();

        let resolved = method_area.get_class("java/lang/Integer").unwrap();
        assert!(!resolved.is_native_stub);
    }

    #[test]
    fn freshly_materialized_native_primitive_constants_are_prepared() {
        let method_area = MethodArea::new();
        let integer = method_area.get_class("java/lang/Integer").unwrap();
        assert!(integer.is_native_stub);
        assert_eq!(
            method_area
                .get_static_exact("java/lang/Integer", "MAX_VALUE", "I")
                .unwrap(),
            Value::Int(i32::MAX)
        );
        assert_eq!(
            method_area
                .get_static_exact("java/lang/Integer", "MIN_VALUE", "I")
                .unwrap(),
            Value::Int(i32::MIN)
        );
    }

    #[test]
    fn materialized_stub_inherits_linked_vtable_and_field_layout() {
        let mut method_area = MethodArea::new();
        let mut parent = LoadedClass::new("test.Parent".to_string());
        parent.modifiers = 0x0001;
        parent
            .field_layout
            .add_field_with_descriptor("state".to_string(), "I".to_string());
        parent.add_vtable_entry(
            "inherited".to_string(),
            "()I".to_string(),
            "test.Parent".to_string(),
        );
        method_area.register_class(parent).unwrap();

        method_area.stub_registry.register_class(StubClass {
            name: "test/NativeChild".to_string(),
            superclass: Some("test/Parent".to_string()),
            interfaces: vec![],
            methods: HashMap::from([(
                "own:()V".to_string(),
                StubMethodDescriptor::instance_method("own", "()V"),
            )]),
        });

        let child = method_area.get_class("test.NativeChild").unwrap();
        assert_eq!(child.field_layout.field_count, 1);
        assert_eq!(
            child
                .field_layout
                .get_offset_with_descriptor("state", "I")
                .unwrap(),
            0
        );
        assert_eq!(
            child
                .resolve_virtual_declaration("inherited", "()I")
                .unwrap()
                .declaring_class
                .as_ref(),
            "test.Parent"
        );
        assert!(child.resolve_virtual_declaration("own", "()V").is_ok());
    }

    #[test]
    fn materialized_deque_contains_transitive_interface_routes() {
        let method_area = MethodArea::new();
        let deque = method_area.get_class("java/util/Deque").unwrap();

        assert!(deque.is_interface);
        assert!(method_area.class_exists("java.util.Queue"));
        assert!(method_area.class_exists("java.util.Collection"));
        assert!(method_area.class_exists("java.lang.Iterable"));
        assert_eq!(
            deque
                .resolve_virtual_declaration("size", "()I")
                .unwrap()
                .declaring_class
                .as_ref(),
            "java/util/Collection"
        );
        assert!(deque.itable.iter().any(|route| {
            route.interface_name.as_ref() == "java.util.Collection"
                && route.method_name.as_ref() == "size"
                && route.descriptor.as_ref() == "()I"
        }));
    }

    #[test]
    fn concurrent_stub_materialization_converges_on_one_definition() {
        let method_area = MethodArea::new();
        let start = Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let method_area = method_area.clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    method_area.get_class("java/util/Deque")
                })
            })
            .collect();

        for thread in threads {
            let deque = thread.join().expect("materialization thread").unwrap();
            assert!(deque.resolve_virtual_declaration("size", "()I").is_ok());
        }
        let published = method_area.all_classes();
        assert!(published.contains_key("java.util.Deque"));
        assert!(published.contains_key("java.util.Queue"));
        assert!(published.contains_key("java.util.Collection"));
        assert!(published.contains_key("java.lang.Iterable"));
    }

    #[test]
    fn failed_stub_link_does_not_publish_the_child() {
        let mut method_area = MethodArea::new();
        method_area.stub_registry.register_class(StubClass {
            name: "test/BrokenNative".to_string(),
            superclass: Some("missing/Superclass".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        let error = method_area.get_class("test.BrokenNative").unwrap_err();
        assert!(matches!(error, VMError::NoClassDefFoundError(_)));
        assert!(!method_area.class_exists("test.BrokenNative"));
    }

    #[test]
    fn native_stub_then_dotted_user_definition_is_a_duplicate() {
        let method_area = MethodArea::new();
        assert!(
            method_area
                .get_class("java/lang/Integer")
                .unwrap()
                .is_native_stub
        );
        let error = method_area
            .register_class(LoadedClass::new("java.lang.Integer".to_string()))
            .unwrap_err();
        assert!(matches!(error, VMError::IncompatibleClassChangeError(_)));
        assert!(
            method_area
                .get_class("java.lang.Integer")
                .unwrap()
                .is_native_stub
        );
    }

    #[test]
    fn slash_and_dot_names_conflict_within_one_batch() {
        let method_area = MethodArea::new();
        let error = method_area
            .register_classes(vec![
                LoadedClass::new("example/Duplicate".to_string()),
                LoadedClass::new("example.Duplicate".to_string()),
            ])
            .unwrap_err();
        assert!(matches!(error, VMError::IncompatibleClassChangeError(_)));
        assert!(!method_area.class_exists("example.Duplicate"));
    }

    #[test]
    fn static_state_has_one_namespace_for_slash_and_dot_names() {
        let method_area = MethodArea::new();
        let mut class = LoadedClass::new("example.Counter".to_string());
        class.declared_fields = Arc::from([FieldDeclaration::new("VALUE", "I", 0x0009)]);
        method_area.register_class(class).unwrap();

        assert_eq!(
            method_area.get_static("example/Counter", "VALUE").unwrap(),
            Value::Int(0)
        );
        method_area
            .put_static("example/Counter", "VALUE", Value::Int(42))
            .unwrap();
        assert_eq!(
            method_area.get_static("example.Counter", "VALUE").unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn constant_preparation_and_clinit_write_share_one_static_slot() {
        let method_area = MethodArea::new();
        let mut class = LoadedClass::new("example.Constant".to_string());
        class.declared_fields = Arc::from([FieldDeclaration::new("VALUE", "I", 0x0019)]);
        class.constant_field_values = Arc::from([ConstantFieldValue {
            declaration_index: 0,
            value: FieldConstantValue::Int(41),
        }]);
        method_area.register_class(class).unwrap();
        let mut heap = Heap::new(1024 * 1024);

        method_area
            .prepare_constant_fields("example.Constant", &mut heap)
            .unwrap();
        assert_eq!(
            method_area
                .get_static_exact("example.Constant", "VALUE", "I")
                .unwrap(),
            Value::Int(41)
        );
        method_area
            .put_static_exact("example.Constant", "VALUE", "I", Value::Int(42))
            .unwrap();
        assert_eq!(
            method_area
                .get_static_exact("example.Constant", "VALUE", "I")
                .unwrap(),
            Value::Int(42)
        );
    }

    #[test]
    fn single_class_publication_rejects_duplicates_without_mutating_static_state() {
        let method_area = MethodArea::new();
        let mut first = LoadedClass::new("example.Duplicate".to_string());
        first.declared_fields = Arc::from([FieldDeclaration::new("VALUE", "I", 0x0009)]);
        method_area.register_class(first).unwrap();

        let mut replacement = LoadedClass::new("example.Duplicate".to_string());
        replacement.declared_fields = Arc::from([FieldDeclaration::new("OTHER", "J", 0x0009)]);
        let error = method_area.register_class(replacement).unwrap_err();
        assert!(matches!(error, VMError::IncompatibleClassChangeError(_)));
        assert!(method_area.get_static("example.Duplicate", "VALUE").is_ok());
        assert!(method_area
            .get_static("example.Duplicate", "OTHER")
            .is_err());
    }

    #[test]
    fn test_loaded_class() {
        let mut class = LoadedClass::new("com.example.User".to_string());
        assert_eq!(&*class.name, "com.example.User");
        assert_eq!(class.init_state, ClassInitState::NotInitialized);
        assert!(!class.is_interface);

        class.set_superclass("java.lang.Object".to_string());
        assert_eq!(class.superclass, Some("java.lang.Object".to_string()));

        class.add_vtable_entry(
            "getName".to_string(),
            "()Ljava/lang/String;".to_string(),
            "com.example.User".to_string(),
        );
        assert!(class
            .get_vtable_index("getName", "()Ljava/lang/String;")
            .is_ok());
        assert!(class.get_vtable_index("missing", "()V").is_err());
    }

    #[test]
    fn test_method_area() {
        let method_area = MethodArea::new();
        let mut class = LoadedClass::new("com.example.Test".to_string());
        class.init_state = ClassInitState::Initialized;

        method_area.register_class(class).unwrap();
        assert!(method_area.class_exists("com.example.Test"));
        assert!(!method_area.class_exists("com.example.Missing"));

        let loaded = method_area.get_class("com.example.Test").unwrap();
        assert_eq!(loaded.init_state, ClassInitState::Initialized);

        assert!(method_area.get_class("com.example.Missing").is_err());
    }

    #[test]
    fn test_method_area_thread_safety() {
        let method_area = MethodArea::new();
        let class = LoadedClass::new("com.example.Shared".to_string());
        method_area.register_class(class).unwrap();

        let ma = method_area.clone();
        assert!(ma.class_exists("com.example.Shared"));
    }
}
