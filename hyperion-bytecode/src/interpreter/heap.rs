// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM heap storage for objects and arrays

use super::errors::{VMError, VMResult};
use super::value::{HeapId, ObjectRef, Value};
use bigdecimal::BigDecimal;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

/// Format a floating-point value the way Java's `Double.toString` does for the
/// common cases: a whole number keeps a trailing `.0` (Java prints `1.0`, Rust
/// prints `1`), while fractional values use Rust's shortest round-trip form.
pub(crate) fn format_java_float(d: f64) -> String {
    if d.is_nan() {
        return "NaN".to_string();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if d == d.trunc() && d.abs() < 1e16 {
        format!("{d:.1}")
    } else {
        d.to_string()
    }
}

/// Like [`format_java_float`] but for `f32`: a whole value keeps a trailing
/// `.0`, while fractional values use `f32`'s shortest round-trip form — which
/// matches Java's `Float.toString` (e.g. `0.1f` → `"0.1"`, not the widened
/// `f64` rendering `"0.10000000149011612"`).
pub(crate) fn format_java_f32(f: f32) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if f == f.trunc() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

/// Natural ordering (`Comparable.compareTo`) over the common element types,
/// reading the heap for Strings/numeric wrappers. Used to give TreeMap/TreeSet
/// their sorted iteration order. Incomparable elements compare equal.
pub(crate) fn natural_cmp(heap: &Heap, a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let as_str = |v: &Value| -> Option<String> {
        if let Value::Reference(ObjectRef::Object(id)) = v {
            if heap.get_object_class_name(*id).ok().as_deref() == Some("java/lang/String") {
                return heap.get_string(*id).ok();
            }
        }
        None
    };
    if let (Some(sa), Some(sb)) = (as_str(a), as_str(b)) {
        return crate::interpreter::natives::java_string_compare(&sa, &sb).cmp(&0);
    }
    let unbox = |v: &Value| -> Option<Value> {
        match v {
            Value::Int(_) | Value::Long(_) | Value::Double(_) | Value::Float(_) => Some(v.clone()),
            Value::Reference(ObjectRef::Object(id)) => heap.get_field(*id, 0).ok().flatten(),
            _ => None,
        }
    };
    match (unbox(a), unbox(b)) {
        (Some(Value::Int(x)), Some(Value::Int(y))) => x.cmp(&y),
        (Some(Value::Long(x)), Some(Value::Long(y))) => x.cmp(&y),
        (Some(Value::Int(x)), Some(Value::Long(y))) => i64::from(x).cmp(&y),
        (Some(Value::Long(x)), Some(Value::Int(y))) => x.cmp(&i64::from(y)),
        (Some(Value::Float(x)), Some(Value::Float(y))) => java_float_cmp(x, y),
        (Some(Value::Double(x)), Some(Value::Double(y))) => java_double_cmp(x, y),
        (Some(Value::Float(x)), Some(Value::Double(y))) => java_double_cmp(f64::from(x), y),
        (Some(Value::Double(x)), Some(Value::Float(y))) => java_double_cmp(x, f64::from(y)),
        (Some(x), Some(y)) => match (numeric_as_f64(&x), numeric_as_f64(&y)) {
            (Some(x), Some(y)) => java_double_cmp(x, y),
            _ => Ordering::Equal,
        },
        _ => Ordering::Equal,
    }
}

fn numeric_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(f64::from(*value)),
        Value::Long(value) => Some(*value as f64),
        Value::Float(value) => Some(f64::from(*value)),
        Value::Double(value) => Some(*value),
        _ => None,
    }
}

fn java_float_cmp(left: f32, right: f32) -> std::cmp::Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => left.total_cmp(&right),
    }
}

fn java_double_cmp(left: f64, right: f64) -> std::cmp::Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => left.total_cmp(&right),
    }
}

/// Maximum array size according to JVM specification
///
/// The JVM spec defines maximum array length as `Integer.MAX_VALUE - 8`.
/// The "-8" accounts for array header overhead in most JVM implementations.
/// This prevents DoS attacks via massive array allocations.
const MAX_ARRAY_SIZE: i32 = i32::MAX - 8;

/// Represents a Java class (referenced in heap entries)
#[derive(Clone, Debug)]
pub struct ClassRef {
    pub name: String,
    pub instance_field_count: usize,
    pub static_field_count: usize,
}

/// A Java object instance on the heap
#[derive(Clone, Debug)]
pub struct ObjectInstance {
    /// Class of this object
    pub class: ClassRef,
    /// Field values (indexed by field offset)
    pub fields: Vec<Option<Value>>,
    /// VM-owned state for `Throwable` objects. Keeping this separate from Java
    /// field offsets avoids aliasing user fields in subclasses of our compact
    /// JDK stubs and gives synthetic exceptions the same representation as
    /// explicitly constructed ones.
    pub throwable: Option<Box<ThrowableData>>,
}

/// The subset of `java.lang.Throwable` state needed by the interpreter.
///
/// `cause_initialized` distinguishes the constructor's unset-cause sentinel
/// from an explicitly null cause, as required by `Throwable.initCause`.
#[derive(Clone, Debug)]
pub struct ThrowableData {
    pub detail_message: ObjectRef,
    pub cause: ObjectRef,
    pub cause_initialized: bool,
}

impl Default for ThrowableData {
    fn default() -> Self {
        Self {
            detail_message: ObjectRef::Null,
            cause: ObjectRef::Null,
            cause_initialized: false,
        }
    }
}

/// An array instance on the heap
#[derive(Clone, Debug)]
pub struct ArrayInstance {
    /// Element type of the array
    pub element_type: ArrayElementType,
    /// Array data
    pub data: ArrayData,
}

/// Type of elements in an array
#[derive(Clone, Debug)]
pub enum ArrayElementType {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    Reference(String), // Class name for object arrays
}

impl ArrayElementType {
    /// Size in bytes for memory accounting
    pub fn size(&self) -> usize {
        match self {
            ArrayElementType::Boolean | ArrayElementType::Byte => 1,
            ArrayElementType::Char | ArrayElementType::Short => 2,
            ArrayElementType::Int | ArrayElementType::Float => 4,
            ArrayElementType::Long | ArrayElementType::Double => 8,
            ArrayElementType::Reference(_) => 8, // On 64-bit systems
        }
    }

    /// Get the type descriptor for this element type (e.g., "I" for int, "Ljava/lang/String;" for String)
    pub fn type_descriptor(&self) -> String {
        match self {
            ArrayElementType::Boolean => "Z".to_string(),
            ArrayElementType::Byte => "B".to_string(),
            ArrayElementType::Char => "C".to_string(),
            ArrayElementType::Short => "S".to_string(),
            ArrayElementType::Int => "I".to_string(),
            ArrayElementType::Long => "J".to_string(),
            ArrayElementType::Float => "F".to_string(),
            ArrayElementType::Double => "D".to_string(),
            ArrayElementType::Reference(class_name) if class_name.starts_with('[') => {
                class_name.clone()
            }
            ArrayElementType::Reference(class_name) => format!("L{};", class_name),
        }
    }

    /// Get the array type descriptor for this element type (e.g., "[I" for int[], "[Ljava/lang/String;" for String[])
    pub fn array_type_descriptor(&self) -> String {
        format!("[{}", self.type_descriptor())
    }
}

/// Array data storage
#[derive(Clone, Debug)]
pub enum ArrayData {
    Boolean(Vec<bool>),
    Byte(Vec<i8>),
    Char(Vec<u16>),
    Short(Vec<i16>),
    Int(Vec<i32>),
    Long(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    Reference(Vec<ObjectRef>),
}

impl ArrayData {
    /// Get the length of the array
    pub fn len(&self) -> usize {
        match self {
            ArrayData::Boolean(v) => v.len(),
            ArrayData::Byte(v) => v.len(),
            ArrayData::Char(v) => v.len(),
            ArrayData::Short(v) => v.len(),
            ArrayData::Int(v) => v.len(),
            ArrayData::Long(v) => v.len(),
            ArrayData::Float(v) => v.len(),
            ArrayData::Double(v) => v.len(),
            ArrayData::Reference(v) => v.len(),
        }
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Estimate memory usage in bytes
    pub fn memory_estimate(&self) -> usize {
        match self {
            ArrayData::Boolean(v) => v.len(),
            ArrayData::Byte(v) => v.len(),
            ArrayData::Char(v) => v.len() * 2,
            ArrayData::Short(v) => v.len() * 2,
            ArrayData::Int(v) => v.len() * 4,
            ArrayData::Long(v) => v.len() * 8,
            ArrayData::Float(v) => v.len() * 4,
            ArrayData::Double(v) => v.len() * 8,
            ArrayData::Reference(v) => v.len() * 8,
        }
    }

    fn try_clone(&self) -> VMResult<Self> {
        fn clone_vec<T: Clone>(source: &[T]) -> VMResult<Vec<T>> {
            let mut cloned = Vec::new();
            cloned
                .try_reserve_exact(source.len())
                .map_err(|_| VMError::out_of_memory())?;
            cloned.extend_from_slice(source);
            Ok(cloned)
        }

        Ok(match self {
            ArrayData::Boolean(values) => ArrayData::Boolean(clone_vec(values)?),
            ArrayData::Byte(values) => ArrayData::Byte(clone_vec(values)?),
            ArrayData::Char(values) => ArrayData::Char(clone_vec(values)?),
            ArrayData::Short(values) => ArrayData::Short(clone_vec(values)?),
            ArrayData::Int(values) => ArrayData::Int(clone_vec(values)?),
            ArrayData::Long(values) => ArrayData::Long(clone_vec(values)?),
            ArrayData::Float(values) => ArrayData::Float(clone_vec(values)?),
            ArrayData::Double(values) => ArrayData::Double(clone_vec(values)?),
            ArrayData::Reference(values) => ArrayData::Reference(clone_vec(values)?),
        })
    }
}

fn try_zero_filled_vec<T: Clone>(length: usize, value: T) -> VMResult<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| VMError::out_of_memory())?;
    values.resize(length, value);
    Ok(values)
}

/// Java collection data stored on the heap
#[derive(Clone, Debug)]
pub enum CollectionData {
    /// ArrayList - dynamic array of values
    ArrayList(Vec<Value>),
    /// Map key-value storage with O(1) lookup and stable insertion order.
    /// HashMap does not promise that order, while LinkedHashMap requires it.
    HashMap(IndexMap<Value, Value>),
    /// HashSet - unique value storage (O(1) lookup)
    HashSet(std::collections::HashSet<Value>),
    /// LinkedList - doubly-linked list
    LinkedList(Vec<Value>),
    /// An iterator over a snapshot of a collection's elements, with a cursor.
    /// Snapshotting at `iterator()` time keeps `hasNext`/`next` simple and
    /// decouples the iterator from later mutation of the source collection.
    Iterator { items: Vec<Value>, cursor: usize },
    /// A lazy Groovy IntRange iterator. Keeping the cursor and exclusive
    /// terminal as i64 avoids allocating a potentially multi-gigabyte snapshot
    /// and safely represents the sentinel beyond either i32 boundary.
    IntRangeIterator {
        current: i64,
        terminal: i64,
        step: i64,
    },
}

/// Thread state enum matching java.lang.Thread.State
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// Thread has not yet started
    New,
    /// Thread is executing
    Runnable,
    /// Thread is blocked waiting for monitor
    Blocked,
    /// Thread is waiting indefinitely
    Waiting,
    /// Thread is waiting with timeout
    TimedWaiting,
    /// Thread has terminated
    Terminated,
}

/// Thread data stored on the heap
#[derive(Clone, Debug)]
pub struct ThreadData {
    /// Unique thread ID (assigned at construction)
    pub thread_id: i64,
    /// Name of the thread
    pub name: String,
    /// Priority (1-10, default 5)
    pub priority: i32,
    /// Whether this is a daemon thread
    pub is_daemon: bool,
    /// Current thread state
    pub state: ThreadState,
    /// Runnable target (if provided via constructor)
    pub target: ObjectRef,
    /// Whether the thread has been interrupted
    pub interrupted: bool,
}

impl CollectionData {
    fn memory_estimate(&self) -> usize {
        match self {
            CollectionData::ArrayList(v) => 32 + v.len() * 16,
            CollectionData::HashMap(v) => 64 + v.len() * 32,
            CollectionData::HashSet(v) => 32 + v.len() * 16,
            CollectionData::LinkedList(v) => 32 + v.len() * 24,
            CollectionData::Iterator { items, .. } => 32 + items.len() * 16,
            CollectionData::IntRangeIterator { .. } => 32,
        }
    }
}

/// Collection instance on the heap
#[derive(Clone, Debug)]
pub struct CollectionInstance {
    /// Collection class name
    pub class_name: String,
    /// Collection data
    pub data: CollectionData,
}

/// Class object instance (java.lang.Class)
#[derive(Clone, Debug)]
pub struct ClassInstance {
    /// The class name this Class object represents (e.g., "java.lang.String")
    pub represented_class: String,
}

/// Heap entry (object, array, collection, class, or thread)
pub enum HeapEntry {
    Object(ObjectInstance),
    Array(ArrayInstance),
    Collection(CollectionInstance),
    Class(ClassInstance),
    Thread(ThreadData),
}

/// One admitted guest allocation. `charged_bytes` is the authoritative ledger
/// used by sweep; it changes only through transactional heap mutation helpers.
/// Recomputing an entry's current capacity during GC would let uncharged growth
/// subtract memory that was never admitted.
struct HeapAllocation {
    entry: HeapEntry,
    charged_bytes: usize,
}

impl HeapEntry {
    /// Estimate memory usage
    fn memory_estimate(&self) -> usize {
        match self {
            HeapEntry::Object(object) => {
                std::mem::size_of::<ObjectInstance>()
                    + object.class.name.capacity()
                    + object.fields.capacity() * std::mem::size_of::<Option<Value>>()
                    + object
                        .throwable
                        .as_ref()
                        .map_or(0, |_| std::mem::size_of::<ThrowableData>())
            }
            HeapEntry::Array(arr) => 32 + arr.data.memory_estimate(),
            HeapEntry::Collection(coll) => coll.data.memory_estimate(),
            HeapEntry::Class(c) => 32 + c.represented_class.len(),
            HeapEntry::Thread(t) => 64 + t.name.capacity(),
        }
    }
}

/// Managed heap for objects and arrays
/// What decides `equals()` for a string or boxed map or set key.
#[derive(Clone, PartialEq, Eq, Hash)]
enum KeyContent {
    Text(Arc<str>),
    /// The wrapper class and primitive bits, as `boxed_key_content` gives them.
    Boxed(String, i64),
}

pub struct Heap {
    /// Storage for all objects and arrays
    objects: HashMap<HeapId, HeapAllocation>,
    /// Dynamic per-object properties (used for Groovy Script getProperty/setProperty).
    dynamic_properties: HashMap<HeapId, HashMap<String, Value>>,
    /// Next available heap ID
    next_id: u32,
    /// String pool for interned strings, keyed by contents
    string_pool: HashMap<Arc<str>, HeapId>,
    /// For each map or set, the key it holds for each string content or boxed value. A
    /// key equal to one of these is stored and looked up as that one, so equal strings and
    /// boxes are one key, as `equals()` says, while `==` stays identity.
    container_keys: HashMap<HeapId, HashMap<KeyContent, HeapId>>,
    /// Contents of each interned String object, for reads by heap id
    string_values: HashMap<HeapId, Arc<str>>,
    /// Class pool for interned Class objects
    class_pool: HashMap<String, HeapId>,
    /// Singleton constants of JDK enums that the stubs cannot declare as fields,
    /// keyed by enum class and constant name.
    enum_constants: HashMap<(String, String), HeapId>,
    /// For each map or set, its user-object keys by `hashCode()`. A key is compared with
    /// these by `equals()`, so equal user objects are one key.
    container_user_keys: HashMap<HeapId, HashMap<i32, Vec<HeapId>>>,
    /// `Integer.valueOf` cache for `-128..=127` (JLS 5.1.7), so equal small
    /// boxed Integers are identity-equal (needed for set/map dedup correctness).
    integer_cache: HashMap<i32, HeapId>,
    /// Memory limit (bytes)
    memory_limit: usize,
    /// Approximate memory used (bytes)
    memory_used: usize,
    /// VM reserve occupied by preallocated exceptions. This is real memory but
    /// deliberately does not consume the guest heap limit: these objects must
    /// remain available precisely when ordinary allocation cannot proceed.
    emergency_memory_used: usize,
    /// Permanently rooted emergency `OutOfMemoryError` and
    /// `StackOverflowError` instances.
    emergency_exceptions: [HeapId; 2],
    /// First exception object that made a class erroneous. Class-init state is
    /// shared metadata, but heap identities belong to a particular heap; keep
    /// the association here so cloning a MethodArea cannot create dangling
    /// cross-heap references.
    class_init_failures: HashMap<String, ObjectRef>,
    /// Classes whose ConstantValue slots have been assigned in this heap.
    /// String constants are heap identities, so preparation cannot live in a
    /// clonable MethodArea independently of the heap.
    prepared_constant_classes: HashSet<String>,
    /// Captured program output (everything written to `System.out`). Accumulated
    /// in addition to the process stdout so embedders/tests can read what a run
    /// printed without scraping the real stdout. `None` until first write.
    captured_stdout: Option<String>,
    /// Captured `System.err` output, same contract as `captured_stdout`.
    captured_stderr: Option<String>,
    /// When true, `System.out`/`.err` writes are buffered only (not echoed to the
    /// process stdout/stderr). Off by default to preserve CLI behavior.
    suppress_console: bool,
}

impl Heap {
    /// Create a new heap with the specified size limit
    pub fn new(memory_limit: usize) -> Self {
        let mut heap = Heap {
            objects: HashMap::new(),
            dynamic_properties: HashMap::new(),
            // 0 is reserved for potential use; 1 and 2 are consumed by the
            // unmetered emergency reserve below.
            next_id: 3,
            string_pool: HashMap::new(),
            container_keys: HashMap::new(),
            string_values: HashMap::new(),
            class_pool: HashMap::new(),
            enum_constants: HashMap::new(),
            container_user_keys: HashMap::new(),
            integer_cache: HashMap::new(),
            memory_limit,
            memory_used: 0,
            emergency_memory_used: 0,
            emergency_exceptions: [HeapId(1), HeapId(2)],
            class_init_failures: HashMap::new(),
            prepared_constant_classes: HashSet::new(),
            captured_stdout: None,
            captured_stderr: None,
            suppress_console: false,
        };
        for (id, class_name) in [
            (heap.emergency_exceptions[0], "java/lang/OutOfMemoryError"),
            (heap.emergency_exceptions[1], "java/lang/StackOverflowError"),
        ] {
            let entry = HeapEntry::Object(ObjectInstance {
                class: ClassRef {
                    name: class_name.to_string(),
                    instance_field_count: 0,
                    static_field_count: 0,
                },
                fields: Vec::new(),
                throwable: Some(Box::default()),
            });
            heap.emergency_memory_used += entry.memory_estimate();
            heap.objects.insert(
                id,
                HeapAllocation {
                    entry,
                    charged_bytes: 0,
                },
            );
        }
        heap
    }

    /// Append program output to the capture buffer and, unless console output is
    /// suppressed, echo it to the process stdout/stderr. `text` is the exact bytes
    /// the program wrote (callers add any trailing newline for `println`).
    pub fn write_program_output(&mut self, text: &str, is_err: bool) {
        let buf = if is_err {
            self.captured_stderr.get_or_insert_with(String::new)
        } else {
            self.captured_stdout.get_or_insert_with(String::new)
        };
        buf.push_str(text);
        if !self.suppress_console {
            if is_err {
                eprint!("{text}");
            } else {
                print!("{text}");
            }
        }
    }

    /// Captured `System.out` output so far (empty if nothing was printed).
    pub fn captured_stdout(&self) -> &str {
        self.captured_stdout.as_deref().unwrap_or("")
    }

    /// Captured `System.err` output so far.
    pub fn captured_stderr(&self) -> &str {
        self.captured_stderr.as_deref().unwrap_or("")
    }

    /// Buffer `System.out`/`.err` without echoing to the process console.
    pub fn set_suppress_console(&mut self, suppress: bool) {
        self.suppress_console = suppress;
    }

    pub fn get_dynamic_property(&self, owner: HeapId, name: &str) -> Option<Value> {
        self.dynamic_properties
            .get(&owner)
            .and_then(|m| m.get(name))
            .cloned()
    }

    pub fn set_dynamic_property(&mut self, owner: HeapId, name: &str, value: Value) {
        self.dynamic_properties
            .entry(owner)
            .or_default()
            .insert(name.to_string(), value);
    }

    /// Allocate an object on the heap
    pub fn allocate_object(&mut self, class: ClassRef) -> VMResult<HeapId> {
        let field_bytes = class
            .instance_field_count
            .checked_mul(std::mem::size_of::<Option<Value>>())
            .ok_or_else(VMError::out_of_memory)?;
        let minimum_estimate = std::mem::size_of::<ObjectInstance>()
            .checked_add(class.name.capacity())
            .and_then(|size| size.checked_add(field_bytes))
            .ok_or_else(VMError::out_of_memory)?;
        self.preflight_allocation(minimum_estimate)?;

        let instance = ObjectInstance {
            fields: try_zero_filled_vec(class.instance_field_count, None)?,
            class,
            throwable: None,
        };

        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Allocate an ordinary Java object and attach the VM-owned Throwable
    /// payload used by both bytecode constructors and synthetic exceptions.
    pub fn allocate_throwable(
        &mut self,
        class: ClassRef,
        detail_message: ObjectRef,
        cause: Option<ObjectRef>,
    ) -> VMResult<ObjectRef> {
        let field_bytes = class
            .instance_field_count
            .checked_mul(std::mem::size_of::<Option<Value>>())
            .ok_or_else(VMError::out_of_memory)?;
        let minimum_estimate = std::mem::size_of::<ObjectInstance>()
            .checked_add(class.name.capacity())
            .and_then(|size| size.checked_add(field_bytes))
            .and_then(|size| size.checked_add(std::mem::size_of::<ThrowableData>()))
            .ok_or_else(VMError::out_of_memory)?;
        self.preflight_allocation(minimum_estimate)?;

        // Build the complete entry after admission so large field layouts use
        // fallible host reservation and the sidecar is charged atomically.
        let id = self.allocate_entry(HeapEntry::Object(ObjectInstance {
            fields: try_zero_filled_vec(class.instance_field_count, None)?,
            class,
            throwable: Some(Box::new(ThrowableData {
                detail_message,
                cause: cause.unwrap_or(ObjectRef::Null),
                cause_initialized: cause.is_some(),
            })),
        }))?;
        Ok(ObjectRef::Object(id))
    }

    /// Replace constructor state on an object admitted with a Throwable
    /// sidecar. Refusing to attach one after admission keeps memory accounting
    /// exact and makes forged constructor calls fail closed.
    /// `None` means the cause has not yet been initialized; `Some(Null)` is an
    /// explicitly null cause.
    pub fn initialize_throwable(
        &mut self,
        id: HeapId,
        detail_message: ObjectRef,
        cause: Option<ObjectRef>,
    ) -> VMResult<()> {
        let object = match self.entry_mut(id) {
            Some(HeapEntry::Object(object)) => object,
            Some(_) => return Err(VMError::type_mismatch("Throwable object", "non-object")),
            None => return Err(VMError::invalid_object_ref()),
        };
        let data = object
            .throwable
            .as_deref_mut()
            .ok_or_else(|| VMError::type_mismatch("Throwable", "ordinary object"))?;
        *data = ThrowableData {
            detail_message,
            cause: cause.unwrap_or(ObjectRef::Null),
            cause_initialized: cause.is_some(),
        };
        Ok(())
    }

    pub fn throwable_data(&self, id: HeapId) -> VMResult<Option<&ThrowableData>> {
        match self.entry(id) {
            Some(HeapEntry::Object(object)) => Ok(object.throwable.as_deref()),
            Some(_) => Err(VMError::type_mismatch("Throwable object", "non-object")),
            None => Err(VMError::invalid_object_ref()),
        }
    }

    pub fn throwable_message(&self, id: HeapId) -> VMResult<ObjectRef> {
        self.throwable_data(id)?
            .map(|data| data.detail_message)
            .ok_or_else(|| VMError::type_mismatch("Throwable", "ordinary object"))
    }

    pub fn throwable_cause(&self, id: HeapId) -> VMResult<ObjectRef> {
        self.throwable_data(id)?
            .map(|data| data.cause)
            .ok_or_else(|| VMError::type_mismatch("Throwable", "ordinary object"))
    }

    /// Set the cause exactly once, matching `Throwable.initCause`'s state
    /// machine. The Java-level validation errors are produced by the native
    /// dispatch layer, which has the receiver reference available.
    pub fn set_throwable_cause(&mut self, id: HeapId, cause: ObjectRef) -> VMResult<bool> {
        let object = match self.entry_mut(id) {
            Some(HeapEntry::Object(object)) => object,
            Some(_) => return Err(VMError::type_mismatch("Throwable object", "non-object")),
            None => return Err(VMError::invalid_object_ref()),
        };
        let Some(data) = object.throwable.as_deref_mut() else {
            return Err(VMError::type_mismatch("Throwable", "ordinary object"));
        };
        if data.cause_initialized {
            return Ok(false);
        }
        data.cause = cause;
        data.cause_initialized = true;
        Ok(true)
    }

    pub fn emergency_out_of_memory(&self) -> ObjectRef {
        ObjectRef::Object(self.emergency_exceptions[0])
    }

    pub fn emergency_stack_overflow(&self) -> ObjectRef {
        ObjectRef::Object(self.emergency_exceptions[1])
    }

    pub(crate) fn intrinsic_root_ids(&self) -> impl Iterator<Item = HeapId> + '_ {
        self.emergency_exceptions
            .iter()
            .copied()
            .chain(self.string_pool.values().copied())
            .chain(self.class_pool.values().copied())
            .chain(self.enum_constants.values().copied())
            .chain(self.integer_cache.values().copied())
            .chain(
                self.class_init_failures
                    .values()
                    .filter_map(|reference| reference.heap_id()),
            )
    }

    /// Record the first Java exception object associated with a class-init
    /// failure. Later active uses attach this same object as the NCDFE cause.
    pub(crate) fn record_class_init_failure(&mut self, class_name: &str, exception: ObjectRef) {
        self.class_init_failures
            .entry(class_name.replace('/', "."))
            .or_insert(exception);
    }

    pub(crate) fn class_init_failure(&self, class_name: &str) -> Option<ObjectRef> {
        self.class_init_failures
            .get(&class_name.replace('/', "."))
            .copied()
    }

    pub(crate) fn constants_prepared(&self, class_name: &str) -> bool {
        self.prepared_constant_classes
            .contains(&class_name.replace('/', "."))
    }

    pub(crate) fn mark_constants_prepared(&mut self, class_name: &str) {
        self.prepared_constant_classes
            .insert(class_name.replace('/', "."));
    }

    pub(crate) fn dynamic_property_values(&self, owner: HeapId) -> impl Iterator<Item = &Value> {
        self.dynamic_properties
            .get(&owner)
            .into_iter()
            .flat_map(|properties| properties.values())
    }

    /// Allocate a boxed Integer object
    pub fn allocate_integer(&mut self, value: i32) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Integer".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Int(value));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Box an int the way `Integer.valueOf` does: values in `-128..=127` come
    /// from a shared cache, so equal small Integers are *identity*-equal (which
    /// is what real Java guarantees and what set/map dedup relies on).
    pub fn box_integer_cached(&mut self, value: i32) -> VMResult<HeapId> {
        if (-128..=127).contains(&value) {
            if let Some(id) = self.integer_cache.get(&value) {
                return Ok(*id);
            }
            let id = self.allocate_integer(value)?;
            self.integer_cache.insert(value, id);
            return Ok(id);
        }
        self.allocate_integer(value)
    }

    /// Allocate a boxed Long object
    pub fn allocate_long(&mut self, value: i64) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Long".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Long(value));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Allocate a boxed Boolean object
    pub fn allocate_boolean(&mut self, value: bool) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Boolean".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Int(if value {
            1
        } else {
            0
        }));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Box a `char` as `java.lang.Character`, whose value is its UTF-16 code unit.
    pub fn allocate_character(&mut self, value: i32) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Character".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Int(value));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Allocate a boxed Double object (value stored in field 0)
    pub fn allocate_double(&mut self, value: f64) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Double".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Double(value));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Allocate a boxed Float object (value stored in field 0)
    pub fn allocate_float(&mut self, value: f32) -> VMResult<HeapId> {
        let class = ClassRef {
            name: "java/lang/Float".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let mut instance = ObjectInstance {
            fields: vec![None; 1],
            class,
            throwable: None,
        };
        instance.fields[0] = Some(crate::interpreter::value::Value::Float(value));
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Allocate a `BigDecimal`, storing its exact plain-string representation.
    ///
    /// The interpreter's heap fields intentionally remain JVM values. Keeping
    /// the decimal digits in an interned String preserves arbitrary precision
    /// without adding a non-JVM value variant to every operand-stack path.
    pub fn allocate_big_decimal(&mut self, value: &BigDecimal) -> VMResult<HeapId> {
        let digits = self.intern_string(&value.to_plain_string())?;
        let class = ClassRef {
            name: "java/math/BigDecimal".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let instance = ObjectInstance {
            fields: vec![Some(Value::Reference(digits))],
            class,
            throwable: None,
        };
        self.allocate_entry(HeapEntry::Object(instance))
    }

    /// Read the exact value stored by [`Self::allocate_big_decimal`].
    pub fn get_big_decimal(&self, heap_id: HeapId) -> VMResult<BigDecimal> {
        if self.get_object_class_name(heap_id)? != "java/math/BigDecimal" {
            return Err(VMError::type_mismatch("BigDecimal", "other object"));
        }
        let Some(Value::Reference(ObjectRef::Object(digits_id))) = self.get_field(heap_id, 0)?
        else {
            return Err(VMError::internal("BigDecimal is missing its digits"));
        };
        let digits = self.get_string(digits_id)?;
        BigDecimal::from_str(&digits)
            .map_err(|_| VMError::internal("BigDecimal contains invalid digits"))
    }

    /// Get the value from a boxed Integer object
    pub fn get_boxed_int(&self, heap_id: HeapId) -> VMResult<i32> {
        if let Some(entry) = self.entry(heap_id) {
            match entry {
                HeapEntry::Object(instance) => {
                    if instance.class.name == "java/lang/Integer" {
                        if let Some(Some(crate::interpreter::value::Value::Int(value))) =
                            instance.fields.first()
                        {
                            return Ok(*value);
                        }
                    }
                    Err(VMError::TypeMismatch {
                        expected: "java/lang/Integer".to_string(),
                        got: instance.class.name.clone(),
                    })
                }
                _ => Err(VMError::TypeMismatch {
                    expected: "Integer object".to_string(),
                    got: "non-object".to_string(),
                }),
            }
        } else {
            Err(VMError::invalid_object_ref())
        }
    }

    /// Get the value from a boxed Long object
    pub fn get_boxed_long(&self, heap_id: HeapId) -> VMResult<i64> {
        if let Some(entry) = self.entry(heap_id) {
            match entry {
                HeapEntry::Object(instance) => {
                    if instance.class.name == "java/lang/Long" {
                        if let Some(Some(crate::interpreter::value::Value::Long(value))) =
                            instance.fields.first()
                        {
                            return Ok(*value);
                        }
                    }
                    Err(VMError::TypeMismatch {
                        expected: "java/lang/Long".to_string(),
                        got: instance.class.name.clone(),
                    })
                }
                _ => Err(VMError::TypeMismatch {
                    expected: "Long object".to_string(),
                    got: "non-object".to_string(),
                }),
            }
        } else {
            Err(VMError::invalid_object_ref())
        }
    }

    /// Allocate an array on the heap
    pub fn allocate_array(
        &mut self,
        element_type: ArrayElementType,
        length: i32,
    ) -> VMResult<HeapId> {
        if length < 0 {
            return Err(VMError::negative_array_size());
        }

        // Enforce maximum array size according to JVM specification
        // This prevents DoS attacks via massive allocations (e.g., `new int[Integer.MAX_VALUE]`)
        if length > MAX_ARRAY_SIZE {
            return Err(VMError::out_of_memory());
        }

        // Check for overflow before allocating: element_type.size() * length as usize
        let element_size = element_type.size();
        let length_usize = length as usize;

        // Check for multiplication overflow
        let byte_size = length_usize
            .checked_mul(element_size)
            .ok_or_else(VMError::out_of_memory)?;

        // Ensure the result fits in isize (maximum allocation size on most systems)
        if byte_size > isize::MAX as usize {
            return Err(VMError::out_of_memory());
        }

        // Admit the allocation before asking the host allocator for the backing
        // buffer. Constructing the Vec first would let a tiny guest heap trigger
        // a multi-gigabyte transient host allocation only to reject it below.
        let memory_estimate = 32usize
            .checked_add(byte_size)
            .ok_or_else(VMError::out_of_memory)?;
        self.preflight_allocation(memory_estimate)?;

        // `try_reserve_exact` turns host allocation failure into Java OOME
        // instead of relying on Vec's infallible allocation path.
        let data = match &element_type {
            ArrayElementType::Boolean => {
                ArrayData::Boolean(try_zero_filled_vec(length_usize, false)?)
            }
            ArrayElementType::Byte => ArrayData::Byte(try_zero_filled_vec(length_usize, 0i8)?),
            ArrayElementType::Char => ArrayData::Char(try_zero_filled_vec(length_usize, 0u16)?),
            ArrayElementType::Short => ArrayData::Short(try_zero_filled_vec(length_usize, 0i16)?),
            ArrayElementType::Int => ArrayData::Int(try_zero_filled_vec(length_usize, 0i32)?),
            ArrayElementType::Long => ArrayData::Long(try_zero_filled_vec(length_usize, 0i64)?),
            ArrayElementType::Float => ArrayData::Float(try_zero_filled_vec(length_usize, 0.0f32)?),
            ArrayElementType::Double => {
                ArrayData::Double(try_zero_filled_vec(length_usize, 0.0f64)?)
            }
            ArrayElementType::Reference(_) => {
                ArrayData::Reference(try_zero_filled_vec(length_usize, ObjectRef::Null)?)
            }
        };

        let instance = ArrayInstance { element_type, data };

        self.allocate_entry(HeapEntry::Array(instance))
    }

    /// Internal method to allocate an entry
    fn allocate_entry(&mut self, entry: HeapEntry) -> VMResult<HeapId> {
        let memory_estimate = entry.memory_estimate();
        let memory_used = self.memory_after_allocation(memory_estimate)?;

        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(VMError::out_of_memory)?;
        let id = HeapId(self.next_id);
        self.objects
            .try_reserve(1)
            .map_err(|_| VMError::out_of_memory())?;

        debug_assert!(!self.objects.contains_key(&id));
        self.objects.insert(
            id,
            HeapAllocation {
                entry,
                charged_bytes: memory_estimate,
            },
        );
        self.next_id = next_id;
        self.memory_used = memory_used;
        self.debug_assert_accounting_consistent();
        Ok(id)
    }

    fn preflight_allocation(&self, minimum_bytes: usize) -> VMResult<()> {
        self.memory_after_allocation(minimum_bytes)?;
        self.next_id
            .checked_add(1)
            .ok_or_else(VMError::out_of_memory)?;
        Ok(())
    }

    fn entry(&self, id: HeapId) -> Option<&HeapEntry> {
        self.objects.get(&id).map(|allocation| &allocation.entry)
    }

    fn entry_mut(&mut self, id: HeapId) -> Option<&mut HeapEntry> {
        self.objects
            .get_mut(&id)
            .map(|allocation| &mut allocation.entry)
    }

    fn debug_assert_accounting_consistent(&self) {
        debug_assert_eq!(
            self.memory_used,
            self.objects
                .values()
                .map(|allocation| allocation.charged_bytes)
                .sum::<usize>(),
            "heap memory ledger diverged from admitted allocations"
        );
    }

    fn memory_after_allocation(&self, additional_bytes: usize) -> VMResult<usize> {
        let memory_used = self
            .memory_used
            .checked_add(additional_bytes)
            .ok_or_else(VMError::out_of_memory)?;
        if memory_used > self.memory_limit {
            return Err(VMError::out_of_memory());
        }
        Ok(memory_used)
    }

    /// Get a field value from an object
    pub fn get_field(&self, id: HeapId, field_offset: usize) -> VMResult<Option<Value>> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Object(obj) => {
                if field_offset >= obj.fields.len() {
                    return Err(VMError::invalid_field_offset());
                }
                Ok(obj.fields[field_offset].clone())
            }
            HeapEntry::Array(_) => Err(VMError::type_mismatch("object", "array")),
            HeapEntry::Collection(_) => Err(VMError::type_mismatch("object", "collection")),
            HeapEntry::Class(_) => Err(VMError::type_mismatch("object", "Class")),
            HeapEntry::Thread(_) => Err(VMError::type_mismatch("object", "thread")),
        }
    }

    /// Set a field value in an object
    pub fn set_field(&mut self, id: HeapId, field_offset: usize, value: Value) -> VMResult<()> {
        self.write_field(id, field_offset, value)
    }

    /// Set a field value on an object
    pub fn put_field(&mut self, id: HeapId, field_offset: usize, value: Value) -> VMResult<()> {
        self.write_field(id, field_offset, value)
    }

    /// Write an object field, growing the dynamic field tail without exposing an
    /// unmetered mutable `Vec`. Growth is built off to the side because
    /// `try_reserve_exact` may over-allocate; the old object and its accounting
    /// remain untouched until the actual retained capacity has been admitted.
    fn write_field(&mut self, id: HeapId, field_offset: usize, value: Value) -> VMResult<()> {
        let (field_len, field_capacity) = match self.entry(id) {
            Some(HeapEntry::Object(object)) => (object.fields.len(), object.fields.capacity()),
            Some(HeapEntry::Array(_)) => {
                return Err(VMError::type_mismatch("object", "array"));
            }
            Some(HeapEntry::Collection(_)) => {
                return Err(VMError::type_mismatch("object", "collection"));
            }
            Some(HeapEntry::Class(_)) => {
                return Err(VMError::type_mismatch("object", "Class"));
            }
            Some(HeapEntry::Thread(_)) => {
                return Err(VMError::type_mismatch("object", "thread"));
            }
            None => return Err(VMError::invalid_object_ref()),
        };

        if field_offset < field_len {
            let Some(HeapEntry::Object(object)) = self.entry_mut(id) else {
                unreachable!("validated object disappeared during field write");
            };
            object.fields[field_offset] = Some(value);
            return Ok(());
        }

        let required_len = field_offset
            .checked_add(1)
            .ok_or_else(VMError::out_of_memory)?;
        if required_len <= field_capacity {
            let Some(HeapEntry::Object(object)) = self.entry_mut(id) else {
                unreachable!("validated object disappeared during field write");
            };
            object.fields.resize(required_len, None);
            object.fields[field_offset] = Some(value);
            return Ok(());
        }

        let slot_size = std::mem::size_of::<Option<Value>>();
        let minimum_growth = required_len
            .checked_sub(field_capacity)
            .and_then(|slots| slots.checked_mul(slot_size))
            .ok_or_else(VMError::out_of_memory)?;
        self.memory_after_allocation(minimum_growth)?;

        let mut replacement = try_zero_filled_vec(required_len, None)?;
        let Some(HeapEntry::Object(object)) = self.entry(id) else {
            unreachable!("validated object disappeared during field growth");
        };
        replacement[..field_len].clone_from_slice(&object.fields);
        replacement[field_offset] = Some(value);

        let actual_growth = replacement
            .capacity()
            .checked_sub(field_capacity)
            .and_then(|slots| slots.checked_mul(slot_size))
            .ok_or_else(VMError::out_of_memory)?;
        let new_memory_used = self.memory_after_allocation(actual_growth)?;
        let new_allocation_charge = self
            .objects
            .get(&id)
            .expect("validated object disappeared during field growth")
            .charged_bytes
            .checked_add(actual_growth)
            .ok_or_else(VMError::out_of_memory)?;

        let allocation = self
            .objects
            .get_mut(&id)
            .expect("validated object disappeared during field growth");
        let HeapEntry::Object(object) = &mut allocation.entry else {
            unreachable!("validated object changed type during field growth");
        };
        object.fields = replacement;
        allocation.charged_bytes = new_allocation_charge;
        self.memory_used = new_memory_used;
        self.debug_assert_accounting_consistent();
        Ok(())
    }

    /// Load a value from an array
    pub fn array_load(&self, id: HeapId, index: i32) -> VMResult<Value> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Array(arr) => {
                let idx = index as usize;
                if index < 0 || idx >= arr.data.len() {
                    return Err(VMError::array_index_out_of_bounds(index, arr.data.len()));
                }

                match &arr.data {
                    ArrayData::Boolean(v) => Ok(Value::Int(if v[idx] { 1 } else { 0 })),
                    ArrayData::Byte(v) => Ok(Value::Int(v[idx] as i32)),
                    ArrayData::Char(v) => Ok(Value::Int(v[idx] as i32)),
                    ArrayData::Short(v) => Ok(Value::Int(v[idx] as i32)),
                    ArrayData::Int(v) => Ok(Value::Int(v[idx])),
                    ArrayData::Long(v) => Ok(Value::Long(v[idx])),
                    ArrayData::Float(v) => Ok(Value::Float(v[idx])),
                    ArrayData::Double(v) => Ok(Value::Double(v[idx])),
                    ArrayData::Reference(v) => Ok(Value::Reference(v[idx])),
                }
            }
            HeapEntry::Object(_) => Err(VMError::type_mismatch("array", "object")),
            HeapEntry::Collection(_) => Err(VMError::type_mismatch("array", "collection")),
            HeapEntry::Class(_) => Err(VMError::type_mismatch("array", "Class")),
            HeapEntry::Thread(_) => Err(VMError::type_mismatch("array", "thread")),
        }
    }

    /// Store a value into an array
    pub fn array_store(&mut self, id: HeapId, index: i32, value: Value) -> VMResult<()> {
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Array(arr) => {
                let idx = index as usize;
                if index < 0 || idx >= arr.data.len() {
                    return Err(VMError::array_index_out_of_bounds(index, arr.data.len()));
                }

                // Check type compatibility first
                match (&arr.data, &value) {
                    (ArrayData::Boolean(_), Value::Int(_))
                    | (ArrayData::Byte(_), Value::Int(_))
                    | (ArrayData::Char(_), Value::Int(_))
                    | (ArrayData::Short(_), Value::Int(_))
                    | (ArrayData::Int(_), Value::Int(_))
                    | (ArrayData::Long(_), Value::Long(_))
                    | (ArrayData::Float(_), Value::Float(_))
                    | (ArrayData::Double(_), Value::Double(_))
                    | (ArrayData::Reference(_), Value::Reference(_)) => {
                        // Type is compatible, perform the store
                    }
                    _ => return Err(VMError::type_mismatch("array element", value.type_name())),
                }

                // Now perform the actual store after type check
                match (&mut arr.data, value) {
                    (ArrayData::Boolean(v), Value::Int(n)) => {
                        v[idx] = n != 0;
                    }
                    (ArrayData::Byte(v), Value::Int(n)) => {
                        v[idx] = n as i8;
                    }
                    (ArrayData::Char(v), Value::Int(n)) => {
                        v[idx] = n as u16;
                    }
                    (ArrayData::Short(v), Value::Int(n)) => {
                        v[idx] = n as i16;
                    }
                    (ArrayData::Int(v), Value::Int(n)) => {
                        v[idx] = n;
                    }
                    (ArrayData::Long(v), Value::Long(n)) => {
                        v[idx] = n;
                    }
                    (ArrayData::Float(v), Value::Float(n)) => {
                        v[idx] = n;
                    }
                    (ArrayData::Double(v), Value::Double(n)) => {
                        v[idx] = n;
                    }
                    (ArrayData::Reference(v), Value::Reference(r)) => {
                        v[idx] = r;
                    }
                    _ => unreachable!(), // Type check above prevents this
                }
                Ok(())
            }
            HeapEntry::Object(_) => Err(VMError::type_mismatch("array", "object")),
            HeapEntry::Collection(_) => Err(VMError::type_mismatch("array", "collection")),
            HeapEntry::Class(_) => Err(VMError::type_mismatch("array", "Class")),
            HeapEntry::Thread(_) => Err(VMError::type_mismatch("array", "thread")),
        }
    }

    /// Get array length
    ///
    /// Per JVM spec, array length is returned as i32. Arrays cannot exceed
    /// i32::MAX elements (enforced at allocation time via i32 length parameter).
    pub fn array_length(&self, id: HeapId) -> VMResult<i32> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Array(arr) => {
                // Defense in depth: verify length fits in i32
                // (should always pass since allocation uses i32 length)
                i32::try_from(arr.data.len()).map_err(|_| VMError::out_of_memory())
            }
            HeapEntry::Object(_) => Err(VMError::type_mismatch("array", "object")),
            HeapEntry::Collection(_) => Err(VMError::type_mismatch("array", "collection")),
            HeapEntry::Class(_) => Err(VMError::type_mismatch("array", "Class")),
            HeapEntry::Thread(_) => Err(VMError::type_mismatch("array", "thread")),
        }
    }

    /// Validate an array index without reading or mutating the element.
    ///
    /// Reference stores use this before their assignment-compatibility check
    /// because JVMS `aastore` gives bounds failures precedence over
    /// `ArrayStoreException`.
    pub fn validate_array_index(&self, id: HeapId, index: i32) -> VMResult<()> {
        let length = self.array_length(id)?;
        if index < 0 || index >= length {
            return Err(VMError::ArrayIndexOutOfBounds { index, length });
        }
        Ok(())
    }

    /// Get the element class name of a reference array
    /// Returns None if the array is not a reference array, or the class name if it is
    pub fn array_element_type(&self, id: HeapId) -> VMResult<Option<String>> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Array(arr) => {
                if let ArrayElementType::Reference(class_name) = &arr.element_type {
                    Ok(Some(class_name.clone()))
                } else {
                    Ok(None)
                }
            }
            HeapEntry::Object(_) => Err(VMError::type_mismatch("array", "object")),
            HeapEntry::Collection(_) => Err(VMError::type_mismatch("array", "collection")),
            HeapEntry::Class(_) => Err(VMError::type_mismatch("array", "Class")),
            HeapEntry::Thread(_) => Err(VMError::type_mismatch("array", "thread")),
        }
    }

    /// Get the class name of an object
    /// Returns the fully qualified class name (with slashes like java/lang/String)
    pub fn get_object_class_name(&self, id: HeapId) -> VMResult<String> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Object(obj) => Ok(obj.class.name.clone()),
            // Array class names are their type descriptors, e.g. [I or [Ljava/lang/String;
            HeapEntry::Array(arr) => Ok(arr.element_type.array_type_descriptor()),
            HeapEntry::Collection(coll) => Ok(coll.class_name.clone()),
            HeapEntry::Class(_) => Ok("java/lang/Class".to_string()),
            HeapEntry::Thread(_) => Ok("java/lang/Thread".to_string()),
        }
    }

    /// A String object that is not shared: runtime-computed strings are distinct
    /// objects, as in Java. Only literals and `String.intern()` go through the pool.
    pub fn new_string(&mut self, value: &str) -> VMResult<ObjectRef> {
        let class = ClassRef {
            name: "java/lang/String".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };
        let id = self.allocate_object(class)?;
        self.string_values.insert(id, Arc::from(value));
        Ok(ObjectRef::Object(id))
    }

    /// Give an existing String object its characters. Used by `String` constructors,
    /// which allocate the object before its contents are known.
    pub fn set_string_value(&mut self, id: HeapId, value: &str) -> VMResult<()> {
        if !self.objects.contains_key(&id) {
            return Err(VMError::invalid_object_ref());
        }
        self.string_values.insert(id, Arc::from(value));
        Ok(())
    }

    /// Intern a string (return existing or create new)
    pub fn intern_string(&mut self, value: &str) -> VMResult<ObjectRef> {
        if let Some(&id) = self.string_pool.get(value) {
            return Ok(ObjectRef::Object(id));
        }

        // The contents live in the pool, not in the object's fields.
        // Use JVM internal name format (slash notation)
        let class = ClassRef {
            name: "java/lang/String".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        };

        let id = self.allocate_object(class)?;
        let contents: Arc<str> = Arc::from(value);
        self.string_pool.insert(Arc::clone(&contents), id);
        self.string_values.insert(id, contents);
        Ok(ObjectRef::Object(id))
    }

    /// The user-object keys of `container` with `hash`, to compare a key against.
    pub fn user_key_candidates(&self, container: HeapId, hash: i32) -> Vec<HeapId> {
        self.container_user_keys
            .get(&container)
            .and_then(|keys| keys.get(&hash))
            .cloned()
            .unwrap_or_default()
    }

    /// Record a user object as a key of `container` with `hash`.
    pub fn add_user_key(&mut self, container: HeapId, hash: i32, id: HeapId) {
        self.container_user_keys
            .entry(container)
            .or_default()
            .entry(hash)
            .or_default()
            .push(id);
    }

    /// Forget `key` as a key of `container`, after it was removed from it.
    fn forget_key(&mut self, container: HeapId, key: &Value) {
        let Value::Reference(ObjectRef::Object(id)) = key else {
            return;
        };
        if let Some(content) = self.key_content(*id) {
            if let Some(keys) = self.container_keys.get_mut(&container) {
                keys.remove(&content);
            }
        }
        if let Some(keys) = self.container_user_keys.get_mut(&container) {
            for bucket in keys.values_mut() {
                bucket.retain(|candidate| candidate != id);
            }
        }
    }

    /// Forget every key of `container`, after it was cleared.
    fn forget_keys(&mut self, container: HeapId) {
        self.container_keys.remove(&container);
        self.container_user_keys.remove(&container);
    }

    /// True for the built-in lists (`ArrayList`, `LinkedList`), whose `contains`, `indexOf`
    /// and `remove(Object)` compare elements with `equals()`.
    pub fn is_list_container(&self, id: HeapId) -> bool {
        matches!(
            self.entry(id),
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(_) | CollectionData::LinkedList(_),
                ..
            }))
        )
    }

    /// True for the hash-based maps and sets. Their keys are compared by `equals()`
    /// and `hashCode()`. Lists, and the sorted maps and sets, are not.
    pub fn is_hashed_container(&self, id: HeapId) -> bool {
        matches!(
            self.entry(id),
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(_) | CollectionData::HashSet(_),
                class_name,
                ..
            })) if !class_name.starts_with("java/util/Tree")
        )
    }

    /// The singleton constant `name` (with its `ordinal`) of a JDK enum, such as
    /// `TimeUnit.MILLISECONDS`. The name and ordinal are stored as the enum natives read them.
    pub fn jdk_enum_constant(&mut self, class: &str, name: &str, ordinal: i32) -> VMResult<HeapId> {
        let key = (class.to_string(), name.to_string());
        if let Some(&id) = self.enum_constants.get(&key) {
            return Ok(id);
        }
        let id = self.allocate_object(ClassRef {
            name: class.to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        let text = self.intern_string(name)?;
        self.set_dynamic_property(id, "$enum_name", Value::Reference(text));
        self.set_dynamic_property(id, "$enum_ordinal", Value::Int(ordinal));
        self.enum_constants.insert(key, id);
        Ok(id)
    }

    /// Intern a Class object (return existing or create new)
    /// Takes internal class name (java/lang/String) and converts to external form (java.lang.String)
    pub fn intern_class(&mut self, internal_name: &str) -> VMResult<ObjectRef> {
        // Convert internal name (java/lang/String) to external name (java.lang.String)
        let external_name = internal_name.replace('/', ".");

        // Check cache first
        if let Some(&id) = self.class_pool.get(&external_name) {
            return Ok(ObjectRef::Object(id));
        }

        // Create new Class object on the heap
        let class_instance = ClassInstance {
            represented_class: external_name.clone(),
        };

        let id = self.allocate_entry(HeapEntry::Class(class_instance))?;
        self.class_pool.insert(external_name, id);
        Ok(ObjectRef::Object(id))
    }

    /// Get the class name represented by a Class object
    pub fn get_class_name(&self, id: HeapId) -> VMResult<String> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Class(class_instance) => Ok(class_instance.represented_class.clone()),
            _ => Err(VMError::TypeMismatch {
                expected: "Class object".to_string(),
                got: "non-Class object".to_string(),
            }),
        }
    }

    /// Get memory usage
    pub fn memory_used(&self) -> usize {
        self.memory_used
    }

    /// Fixed VM reserve excluded from the guest heap limit.
    pub fn emergency_memory_used(&self) -> usize {
        self.emergency_memory_used
    }

    /// Get memory limit
    pub fn memory_limit(&self) -> usize {
        self.memory_limit
    }

    /// Check if memory limit exceeded
    pub fn is_out_of_memory(&self) -> bool {
        self.memory_used >= self.memory_limit
    }

    /// Check if GC should run (heap usage > 70%)
    pub fn should_collect(&self) -> bool {
        (self.memory_used as u128) * 10 > (self.memory_limit as u128) * 7
    }

    /// Get all string pool heap IDs (for GC roots)
    pub fn string_pool_ids(&self) -> impl Iterator<Item = HeapId> + '_ {
        self.string_pool.values().copied()
    }

    /// Get all class pool heap IDs (for GC roots)
    pub fn class_pool_ids(&self) -> impl Iterator<Item = HeapId> + '_ {
        self.class_pool.values().copied()
    }

    /// Get string value from a HeapId
    pub fn get_string(&self, id: HeapId) -> VMResult<String> {
        if let Some(contents) = self.string_values.get(&id) {
            return Ok(contents.to_string());
        }
        Err(VMError::TypeMismatch {
            expected: "String object".to_string(),
            got: "non-String object".to_string(),
        })
    }

    /// Get the class of an object
    pub fn get_class(&self, id: HeapId) -> VMResult<ClassRef> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Object(obj) => Ok(obj.class.clone()),
            HeapEntry::Array(arr) => {
                // Return array type descriptor as the "class" name (e.g., "[I", "[Ljava/lang/String;")
                let array_type = arr.element_type.array_type_descriptor();
                Ok(ClassRef {
                    name: array_type,
                    instance_field_count: 0,
                    static_field_count: 0,
                })
            }
            HeapEntry::Collection(coll) => Ok(ClassRef {
                name: coll.class_name.clone(),
                instance_field_count: 0,
                static_field_count: 0,
            }),
            HeapEntry::Class(_) => Ok(ClassRef {
                name: "java/lang/Class".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            }),
            HeapEntry::Thread(_) => Ok(ClassRef {
                name: "java/lang/Thread".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            }),
        }
    }

    // === GC Support Methods ===

    /// Get a heap entry by ID (for GC mark phase)
    pub fn get_entry(&self, id: HeapId) -> Option<&HeapEntry> {
        self.entry(id)
    }

    /// Remove a heap entry and return its size (for GC sweep phase)
    ///
    /// Returns (memory_size, entry) if the ID exists
    pub(crate) fn remove_unmarked(&mut self, id: HeapId) -> Option<(usize, HeapEntry)> {
        // The collector's mark phase centrally roots every heap-owned pool, so
        // this hot path need not rescan those pools for every dead object.
        if self.emergency_exceptions.contains(&id) {
            return None;
        }
        if let Some(allocation) = self.objects.remove(&id) {
            let size = allocation.charged_bytes;
            self.memory_used = self
                .memory_used
                .checked_sub(size)
                .expect("heap memory ledger underflow during sweep");
            self.dynamic_properties.remove(&id);
            self.string_values.remove(&id);
            self.container_keys.remove(&id);
            self.container_user_keys.remove(&id);
            self.debug_assert_accounting_consistent();
            Some((size, allocation.entry))
        } else {
            None
        }
    }

    /// Iterate over all heap IDs (for GC sweep phase)
    pub fn all_ids(&self) -> impl Iterator<Item = HeapId> + '_ {
        self.objects.keys().copied()
    }

    pub(crate) fn guest_object_count(&self) -> usize {
        self.objects
            .len()
            .saturating_sub(self.emergency_exceptions.len())
    }

    // === Collection Storage Methods ===

    /// Allocate an ArrayList on the heap
    pub fn allocate_arraylist(&mut self) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/ArrayList".to_string(),
            data: CollectionData::ArrayList(Vec::new()),
        }))
    }

    /// Shallow-copy an array (the `Object.clone()` semantics for array types):
    /// a new array of the same element type and length with the same elements.
    pub fn clone_array(&mut self, id: HeapId) -> VMResult<HeapId> {
        let cloned = match self.entry(id) {
            Some(HeapEntry::Array(arr)) => {
                let estimate = 32usize
                    .checked_add(arr.data.memory_estimate())
                    .ok_or_else(VMError::out_of_memory)?;
                self.preflight_allocation(estimate)?;
                ArrayInstance {
                    element_type: arr.element_type.clone(),
                    data: arr.data.try_clone()?,
                }
            }
            _ => return Err(VMError::type_mismatch("array", "non-array")),
        };
        self.allocate_entry(HeapEntry::Array(cloned))
    }

    /// Allocate a HashMap on the heap
    pub fn allocate_hashmap(&mut self) -> VMResult<HeapId> {
        self.allocate_map("java/util/HashMap")
    }

    /// Allocate a LinkedHashMap on the heap, retaining its runtime class and
    /// insertion-order iteration contract.
    pub fn allocate_linked_hashmap(&mut self) -> VMResult<HeapId> {
        self.allocate_map("java/util/LinkedHashMap")
    }

    /// Allocate a hash map that reports `class_name` as its runtime class, so a subclass
    /// such as `Properties` keeps the class its constructor is invoked on.
    pub(crate) fn allocate_map(&mut self, class_name: &str) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: class_name.to_string(),
            data: CollectionData::HashMap(IndexMap::new()),
        }))
    }

    /// Allocate a HashSet on the heap
    pub fn allocate_hashset(&mut self) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/HashSet".to_string(),
            data: CollectionData::HashSet(std::collections::HashSet::new()),
        }))
    }

    /// Allocate a TreeMap on the heap (HashMap-backed; iteration is sorted by
    /// the native dispatch layer, which knows the class is a TreeMap).
    pub fn allocate_treemap(&mut self) -> VMResult<HeapId> {
        self.allocate_map("java/util/TreeMap")
    }

    /// Allocate a TreeSet on the heap (HashSet-backed; sorted on iteration).
    pub fn allocate_treeset(&mut self) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/TreeSet".to_string(),
            data: CollectionData::HashSet(std::collections::HashSet::new()),
        }))
    }

    /// Allocate a LinkedList on the heap
    pub fn allocate_linkedlist(&mut self) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/LinkedList".to_string(),
            data: CollectionData::LinkedList(Vec::new()),
        }))
    }

    /// Allocate a `java.util.stream.Stream` on the heap. Streams are modeled
    /// eagerly: a stream is a snapshot of its elements, and each intermediate
    /// operation (filter/map/…) produces a new snapshot. `collection_snapshot`
    /// reads the elements back in order.
    pub fn allocate_stream(&mut self, items: Vec<Value>) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/stream/Stream".to_string(),
            data: CollectionData::ArrayList(items),
        }))
    }

    /// Allocate a `java.util.stream.Collector` marker. The factory that made it
    /// (`Collectors.joining`/`toList`/…) records its kind and arguments as
    /// dynamic properties; `Stream.collect` reads them back.
    pub fn allocate_collector(&mut self) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/stream/Collector".to_string(),
            data: CollectionData::ArrayList(Vec::new()),
        }))
    }

    /// Add element to LinkedList, returns true (always succeeds per List contract)
    pub fn linkedlist_add(&mut self, id: HeapId, value: Value) -> VMResult<bool> {
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::LinkedList(list),
                ..
            }) => {
                list.push(value);
                Ok(true)
            }
            _ => Err(VMError::type_mismatch("LinkedList", "other")),
        }
    }

    /// Snapshot a collection's elements in iteration order. ArrayList/LinkedList
    /// preserve insertion order; HashSet's order is unspecified (as in Java).
    pub fn collection_snapshot(&self, id: HeapId) -> VMResult<Vec<Value>> {
        if let Some(array) = self.backing_array(id) {
            return (0..self.array_length(array)?)
                .map(|index| self.array_load(array, index))
                .collect();
        }
        if let Some(elements) = self.map_view_elements(id)? {
            return Ok(elements);
        }
        match self.entry(id) {
            Some(HeapEntry::Collection(CollectionInstance { data, class_name })) => {
                let mut items: Vec<Value> = match data {
                    CollectionData::ArrayList(v) | CollectionData::LinkedList(v) => v.clone(),
                    CollectionData::HashSet(s) => s.iter().cloned().collect(),
                    CollectionData::HashMap(m) => m.keys().cloned().collect(),
                    CollectionData::Iterator { items, cursor } => items[*cursor..].to_vec(),
                    CollectionData::IntRangeIterator { .. } => {
                        return Err(VMError::type_mismatch("collection", "IntRange iterator"));
                    }
                };
                // TreeSet iterates in natural (sorted) order.
                if class_name == "java/util/TreeSet" {
                    items.sort_by(|a, b| natural_cmp(self, a, b));
                }
                Ok(items)
            }
            _ => Err(VMError::type_mismatch("collection", "other")),
        }
    }

    /// Snapshot a map's key/value pairs. Insertion order is retained for
    /// LinkedHashMap; TreeMap is sorted below; HashMap's observed order remains
    /// an allowed implementation-specific order.
    pub fn map_snapshot(&self, id: HeapId) -> VMResult<Vec<(Value, Value)>> {
        match self.entry(id) {
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(m),
                class_name,
            })) => {
                let mut entries: Vec<(Value, Value)> =
                    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                // TreeMap iterates in natural key order.
                if class_name == "java/util/TreeMap" {
                    entries.sort_by(|(ka, _), (kb, _)| natural_cmp(self, ka, kb));
                }
                Ok(entries)
            }
            _ => Err(VMError::type_mismatch("map", "other")),
        }
    }

    /// True for the built-in lists, sets and maps whose `toString()` lists their contents.
    /// Iterators are excluded: their `toString()` is `Object`'s.
    pub fn is_container(&self, id: HeapId) -> bool {
        matches!(
            self.entry(id),
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(_)
                    | CollectionData::LinkedList(_)
                    | CollectionData::HashSet(_)
                    | CollectionData::HashMap(_),
                ..
            }))
        )
    }

    /// True if the heap id refers to a map-backed collection.
    pub fn is_map(&self, id: HeapId) -> bool {
        matches!(
            self.entry(id),
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(_),
                ..
            }))
        )
    }

    /// Render a heap object the way Java's `toString` would, for the built-in
    /// types whose textual form is well-defined and deterministic: `String`,
    /// the primitive wrapper classes, `StringBuilder`/`StringBuffer`, and the
    /// ordered/keyed collections. Returns `None` for user-defined or unknown
    /// object types, whose `toString` must be invoked by the executor rather
    /// than synthesised here.
    ///
    /// Shared by result display (`VirtualMachine::display_value`) and the
    /// `append` / `String.valueOf` native path so the two never diverge.
    pub fn render_known_object(&self, id: HeapId) -> Option<String> {
        // Enum constants render as their name (Enum.toString default), stored as
        // the `$enum_name` dynamic property by the Enum constructor.
        if let Some(Value::Reference(ObjectRef::Object(nid))) =
            self.get_dynamic_property(id, "$enum_name")
        {
            if let Ok(name) = self.get_string(nid) {
                return Some(name);
            }
        }
        let class = self.get_object_class_name(id).ok()?;
        let field0_int = || match self.get_field(id, 0) {
            Ok(Some(Value::Int(v))) => Some(v),
            _ => None,
        };
        match class.as_str() {
            "java/lang/String" => self.get_string(id).ok(),
            "java/lang/Integer" | "java/lang/Short" | "java/lang/Byte" => {
                field0_int().map(|v| v.to_string())
            }
            "java/lang/Long" => self.get_boxed_long(id).ok().map(|v| v.to_string()),
            "java/lang/Boolean" => field0_int().map(|v| (v != 0).to_string()),
            "java/lang/Character" => field0_int()
                .map(|v| char::from_u32(v as u32).map_or_else(|| v.to_string(), |c| c.to_string())),
            "java/lang/Double" | "java/lang/Float" => match self.get_field(id, 0) {
                Ok(Some(Value::Double(d))) => Some(format_java_float(d)),
                Ok(Some(Value::Float(f))) => Some(format_java_f32(f)),
                _ => None,
            },
            "java/math/BigDecimal" => self
                .get_big_decimal(id)
                .ok()
                .map(|value| value.to_plain_string()),
            "java/lang/StringBuilder" | "java/lang/StringBuffer" => match self.get_field(id, 0) {
                Ok(Some(Value::Reference(ObjectRef::Object(sid)))) => self.get_string(sid).ok(),
                _ => Some(String::new()),
            },
            _ if self.is_map(id) => {
                let entries = self.map_snapshot(id).ok()?;
                let rendered: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}={}", self.render_value(k), self.render_value(v)))
                    .collect();
                Some(format!("{{{}}}", rendered.join(", ")))
            }
            _ => {
                // Lists, sets, and other element-backed collections render as
                // `[a, b, c]`, matching java.util.AbstractCollection.toString.
                let items = self.collection_snapshot(id).ok()?;
                let rendered: Vec<String> = items.iter().map(|v| self.render_value(v)).collect();
                Some(format!("[{}]", rendered.join(", ")))
            }
        }
    }

    /// Render a value as Java's `String.valueOf` would, falling back to an
    /// `internal/class/Name@<id>` handle for objects without a synthesised
    /// `toString` and to element contents for arrays. Recurses through
    /// collections.
    pub fn render_value(&self, value: &Value) -> String {
        match value {
            Value::Int(i) => i.to_string(),
            Value::Long(l) => l.to_string(),
            Value::Float(f) => format_java_f32(*f),
            Value::Double(d) => format_java_float(*d),
            Value::Reference(ObjectRef::Null) => "null".to_string(),
            Value::Reference(ObjectRef::Object(id)) => self
                .render_known_object(*id)
                .unwrap_or_else(|| format!("{}@{}", self.object_class_or_unknown(*id), id.0)),
            Value::Reference(ObjectRef::Array(id)) => {
                let len = self.array_length(*id).unwrap_or(0);
                let parts: Vec<String> = (0..len)
                    .map(|i| {
                        self.array_load(*id, i)
                            .map_or_else(|_| "?".to_string(), |v| self.render_value(&v))
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Reference(ObjectRef::MethodHandle(id)) => format!("MethodHandle@{}", id.0),
            Value::ReturnAddress(addr) => format!("@{addr}"),
            Value::CategoryTwoPlaceholder => "<placeholder>".to_string(),
        }
    }

    fn object_class_or_unknown(&self, id: HeapId) -> String {
        self.get_object_class_name(id)
            .unwrap_or_else(|_| "java/lang/Object".to_string())
    }

    /// Allocate an iterator over a pre-computed element snapshot.
    pub fn allocate_iterator(&mut self, items: Vec<Value>) -> VMResult<HeapId> {
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/Iterator".to_string(),
            data: CollectionData::Iterator { items, cursor: 0 },
        }))
    }

    /// Allocate a lazy iterator for Groovy's inclusive or exclusive IntRange.
    pub fn allocate_int_range_iterator(
        &mut self,
        from: i32,
        to: i32,
        inclusive: bool,
    ) -> VMResult<HeapId> {
        let step = if from <= to { 1_i64 } else { -1_i64 };
        let terminal = if inclusive {
            i64::from(to) + step
        } else {
            i64::from(to)
        };
        self.allocate_entry(HeapEntry::Collection(CollectionInstance {
            class_name: "java/util/Iterator".to_string(),
            data: CollectionData::IntRangeIterator {
                current: i64::from(from),
                terminal,
                step,
            },
        }))
    }

    /// `Iterator.hasNext()` — true while the cursor is before the end.
    pub fn iterator_has_next(&self, id: HeapId) -> VMResult<bool> {
        match self.entry(id) {
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::Iterator { items, cursor },
                ..
            })) => Ok(*cursor < items.len()),
            Some(HeapEntry::Collection(CollectionInstance {
                data:
                    CollectionData::IntRangeIterator {
                        current,
                        terminal,
                        step,
                    },
                ..
            })) => Ok(if *step > 0 {
                *current < *terminal
            } else {
                *current > *terminal
            }),
            _ => Err(VMError::type_mismatch("Iterator", "other")),
        }
    }

    /// `Iterator.next()` — return the element at the cursor and advance it.
    pub fn iterator_next(&mut self, id: HeapId) -> VMResult<Value> {
        let int_range_value = match self.entry_mut(id) {
            Some(HeapEntry::Collection(CollectionInstance {
                data: CollectionData::Iterator { items, cursor },
                ..
            })) => {
                if *cursor >= items.len() {
                    return Err(VMError::internal("Iterator.next past end"));
                }
                let v = items[*cursor].clone();
                *cursor += 1;
                return Ok(v);
            }
            Some(HeapEntry::Collection(CollectionInstance {
                data:
                    CollectionData::IntRangeIterator {
                        current,
                        terminal,
                        step,
                    },
                ..
            })) => {
                let has_next = if *step > 0 {
                    *current < *terminal
                } else {
                    *current > *terminal
                };
                if !has_next {
                    return Err(VMError::internal("Iterator.next past end"));
                }
                let value = i32::try_from(*current)
                    .map_err(|_| VMError::internal("IntRange cursor exceeded i32 bounds"))?;
                *current += *step;
                value
            }
            _ => return Err(VMError::type_mismatch("Iterator", "other")),
        };
        let boxed = self.box_integer_cached(int_range_value)?;
        Ok(Value::Reference(ObjectRef::Object(boxed)))
    }

    /// Add element to ArrayList, returns true (always succeeds per List contract)
    pub fn arraylist_add(&mut self, id: HeapId, value: Value) -> VMResult<bool> {
        if self.view_is_fixed(id) {
            return Err(VMError::UnsupportedOperationException(
                "a view of an array or a map cannot be added to".to_string(),
            ));
        }
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list),
                ..
            }) => {
                list.push(value);
                Ok(true)
            }
            _ => Err(VMError::type_mismatch("ArrayList", "other")),
        }
    }

    /// A List over an array, as `Arrays.asList` returns it. Reads and `set` go to the
    /// array, so the list and the array see each other's writes, and the size is fixed.
    /// The array is kept as the list's dynamic property, so the collector keeps it alive.
    pub fn allocate_array_view(&mut self, array: HeapId) -> VMResult<HeapId> {
        let list = self.allocate_arraylist()?;
        self.set_dynamic_property(
            list,
            "$backing_array",
            Value::Reference(ObjectRef::Array(array)),
        );
        Ok(list)
    }

    /// The array a list views, when the list is an `Arrays.asList` view.
    fn backing_array(&self, id: HeapId) -> Option<HeapId> {
        match self.get_dynamic_property(id, "$backing_array") {
            Some(Value::Reference(ObjectRef::Array(array))) => Some(array),
            _ => None,
        }
    }

    /// A live keySet (or values) view of `map`. It is read from the map each time, so it
    /// sees later puts. Its elements are the map's keys (or values), not copies.
    pub fn allocate_map_view(&mut self, map: HeapId, values: bool) -> VMResult<HeapId> {
        let view = self.allocate_arraylist()?;
        self.set_dynamic_property(
            view,
            "$backing_map",
            Value::Reference(ObjectRef::Object(map)),
        );
        self.set_dynamic_property(view, "$map_view_values", Value::Int(i32::from(values)));
        Ok(view)
    }

    fn backing_map(&self, id: HeapId) -> Option<HeapId> {
        match self.get_dynamic_property(id, "$backing_map") {
            Some(Value::Reference(ObjectRef::Object(map))) => Some(map),
            _ => None,
        }
    }

    /// The elements of a keySet or values view, read from its map now.
    fn map_view_elements(&self, id: HeapId) -> VMResult<Option<Vec<Value>>> {
        let Some(map) = self.backing_map(id) else {
            return Ok(None);
        };
        let values = matches!(
            self.get_dynamic_property(id, "$map_view_values"),
            Some(Value::Int(flag)) if flag != 0
        );
        Ok(Some(
            self.map_snapshot(map)?
                .into_iter()
                .map(|(key, value)| if values { value } else { key })
                .collect(),
        ))
    }

    /// Marks a list as the entrySet of `map`: a copy of its entries, which removes an
    /// entry from the map when it is removed from the list.
    pub fn mark_entry_set(&mut self, list: HeapId, map: HeapId) {
        self.set_dynamic_property(
            list,
            "$entry_set_map",
            Value::Reference(ObjectRef::Object(map)),
        );
    }

    fn entry_set_map(&self, id: HeapId) -> Option<HeapId> {
        match self.get_dynamic_property(id, "$entry_set_map") {
            Some(Value::Reference(ObjectRef::Object(map))) => Some(map),
            _ => None,
        }
    }

    /// A view is fixed: its elements come from another collection, so add and set are
    /// refused. Removal from a map's view removes from the map, and an Arrays.asList
    /// view refuses it, as its size is fixed.
    fn view_is_fixed(&self, id: HeapId) -> bool {
        self.backing_array(id).is_some()
            || self.backing_map(id).is_some()
            || self.entry_set_map(id).is_some()
    }

    fn fixed_size_error() -> VMError {
        VMError::UnsupportedOperationException("an Arrays.asList view has a fixed size".to_string())
    }

    /// `index` checked against the length of the array a view lists.
    fn view_index(&self, array: HeapId, index: i32) -> VMResult<i32> {
        let length = self.array_length(array)?;
        if index < 0 || index >= length {
            return Err(VMError::array_index_out_of_bounds(index, length as usize));
        }
        Ok(index)
    }

    /// Get element from ArrayList by index
    pub fn arraylist_get(&self, id: HeapId, index: i32) -> VMResult<Value> {
        if let Some(array) = self.backing_array(id) {
            let index = self.view_index(array, index)?;
            return self.array_load(array, index);
        }
        if let Some(elements) = self.map_view_elements(id)? {
            return usize::try_from(index)
                .ok()
                .and_then(|index| elements.get(index).cloned())
                .ok_or_else(|| VMError::array_index_out_of_bounds(index, elements.len()));
        }
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list),
                ..
            }) => {
                if index < 0 || index as usize >= list.len() {
                    return Err(VMError::array_index_out_of_bounds(index, list.len()));
                }
                Ok(list[index as usize].clone())
            }
            _ => Err(VMError::type_mismatch("ArrayList", "other")),
        }
    }

    /// Get ArrayList size
    pub fn arraylist_size(&self, id: HeapId) -> VMResult<i32> {
        if let Some(array) = self.backing_array(id) {
            return self.array_length(array);
        }
        if let Some(elements) = self.map_view_elements(id)? {
            return Ok(elements.len() as i32);
        }
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list),
                ..
            }) => Ok(list.len() as i32),
            _ => Err(VMError::type_mismatch("ArrayList", "other")),
        }
    }

    /// Set element at index in ArrayList
    pub fn arraylist_set(&mut self, id: HeapId, index: i32, value: Value) -> VMResult<Value> {
        if let Some(array) = self.backing_array(id) {
            let index = self.view_index(array, index)?;
            let old = self.array_load(array, index)?;
            self.array_store(array, index, value)?;
            return Ok(old);
        }
        if self.view_is_fixed(id) {
            return Err(VMError::UnsupportedOperationException(
                "a keySet or values view cannot be changed through the list".to_string(),
            ));
        }
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list),
                ..
            }) => {
                if index < 0 || index as usize >= list.len() {
                    return Err(VMError::array_index_out_of_bounds(index, list.len()));
                }
                let old = list[index as usize].clone();
                list[index as usize] = value;
                Ok(old)
            }
            _ => Err(VMError::type_mismatch("ArrayList", "other")),
        }
    }

    /// Remove the element at `index` from an `ArrayList` or `LinkedList`.
    pub fn arraylist_remove(&mut self, id: HeapId, index: i32) -> VMResult<Value> {
        if self.backing_array(id).is_some() {
            return Err(Self::fixed_size_error());
        }
        if let Some(map) = self.backing_map(id) {
            // A keySet or values view lists the map's entries in order, so the element at
            // `index` belongs to the entry at `index`.
            let element = self.arraylist_get(id, index)?;
            let (key, _) = self.map_snapshot(map)?.swap_remove(index as usize);
            self.hashmap_remove(map, &key)?;
            return Ok(element);
        }
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        let removed = match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list) | CollectionData::LinkedList(list),
                ..
            }) => {
                if index < 0 || index as usize >= list.len() {
                    return Err(VMError::array_index_out_of_bounds(index, list.len()));
                }
                list.remove(index as usize)
            }
            _ => return Err(VMError::type_mismatch("ArrayList", "other")),
        };
        if let (Some(map), Value::Reference(ObjectRef::Object(map_entry))) =
            (self.entry_set_map(id), &removed)
        {
            let key = self
                .get_field(*map_entry, 0)?
                .unwrap_or(Value::Reference(ObjectRef::Null));
            self.hashmap_remove(map, &key)?;
        }
        Ok(removed)
    }

    /// Clear ArrayList
    pub fn arraylist_clear(&mut self, id: HeapId) -> VMResult<()> {
        if self.backing_array(id).is_some() {
            return Err(Self::fixed_size_error());
        }
        if let Some(map) = self.backing_map(id).or_else(|| self.entry_set_map(id)) {
            self.hashmap_clear(map)?;
            if self.backing_map(id).is_some() {
                return Ok(());
            }
        }
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::ArrayList(list),
                ..
            }) => {
                list.clear();
                Ok(())
            }
            _ => Err(VMError::type_mismatch("ArrayList", "other")),
        }
    }

    /// `equals()` for two elements of a collection, where the runtime can decide it: the
    /// same value, equal strings, or boxes of the same class and primitive. A user object's
    /// own equals() needs the interpreter, so identity decides for it here.
    pub fn same_element(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Reference(ObjectRef::Object(x)), Value::Reference(ObjectRef::Object(y))) => {
                if x == y {
                    return true;
                }
                if let (Some(s), Some(t)) = (self.string_values.get(x), self.string_values.get(y)) {
                    return s == t;
                }
                matches!(
                    (self.boxed_key_content(*x), self.boxed_key_content(*y)),
                    (Some(left), Some(right)) if left == right
                )
            }
            _ => a == b,
        }
    }

    /// The wrapper class and primitive bits that decide a boxed key's equality, or `None`
    /// when the object is not a boxed primitive. Bits follow `equals()`: doubles and
    /// floats compare by their bit pattern, with every NaN equal to every other.
    fn boxed_key_content(&self, id: HeapId) -> Option<(String, i64)> {
        let Some(HeapEntry::Object(instance)) = self.entry(id) else {
            return None;
        };
        if !matches!(
            instance.class.name.as_str(),
            "java/lang/Integer"
                | "java/lang/Long"
                | "java/lang/Short"
                | "java/lang/Byte"
                | "java/lang/Character"
                | "java/lang/Boolean"
                | "java/lang/Double"
                | "java/lang/Float"
        ) {
            return None;
        }
        let bits = match instance.fields.first() {
            Some(Some(Value::Int(v))) => i64::from(*v),
            Some(Some(Value::Long(v))) => *v,
            Some(Some(Value::Double(v))) => {
                let v = if v.is_nan() { f64::NAN } else { *v };
                v.to_bits() as i64
            }
            Some(Some(Value::Float(v))) => {
                let v = if v.is_nan() { f32::NAN } else { *v };
                i64::from(v.to_bits())
            }
            _ => return None,
        };
        Some((instance.class.name.clone(), bits))
    }

    /// What decides `equals()` for a string or boxed key, or `None` for any other object.
    fn key_content(&self, id: HeapId) -> Option<KeyContent> {
        if let Some(text) = self.string_values.get(&id) {
            return Some(KeyContent::Text(Arc::clone(text)));
        }
        self.boxed_key_content(id)
            .map(|(class, bits)| KeyContent::Boxed(class, bits))
    }

    /// The key `container` stores for `key`: the key it already holds that equals it, or
    /// `key` itself, which is then recorded as that key.
    fn canonical_key(&mut self, container: HeapId, key: &Value) -> Value {
        let Value::Reference(ObjectRef::Object(id)) = key else {
            return key.clone();
        };
        match self.key_content(*id) {
            Some(content) => Value::Reference(ObjectRef::Object(
                *self
                    .container_keys
                    .entry(container)
                    .or_default()
                    .entry(content)
                    .or_insert(*id),
            )),
            None => key.clone(),
        }
    }

    /// The key to look `key` up by in `container`, without recording it. When the
    /// container holds no key equal to it, its own identity is the right answer.
    fn existing_key(&self, container: HeapId, key: &Value) -> Value {
        let Value::Reference(ObjectRef::Object(id)) = key else {
            return key.clone();
        };
        match self.key_content(*id).and_then(|content| {
            self.container_keys
                .get(&container)
                .and_then(|keys| keys.get(&content))
        }) {
            Some(canonical) => Value::Reference(ObjectRef::Object(*canonical)),
            None => key.clone(),
        }
    }

    /// Put key-value into HashMap, returns previous value or null
    pub fn hashmap_put(&mut self, id: HeapId, key: Value, value: Value) -> VMResult<Value> {
        let key = self.canonical_key(id, &key);
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => {
                // O(1) insert, returns previous value if any
                match map.insert(key, value) {
                    Some(old) => Ok(old),
                    None => Ok(Value::Reference(ObjectRef::Null)),
                }
            }
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    /// Get value from HashMap by key
    pub fn hashmap_get(&self, id: HeapId, key: &Value) -> VMResult<Value> {
        let key = self.existing_key(id, key);
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => {
                // O(1) lookup
                match map.get(&key) {
                    Some(v) => Ok(v.clone()),
                    None => Ok(Value::Reference(ObjectRef::Null)),
                }
            }
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    /// Remove `key` from a map, keeping the order of the other entries. Returns the
    /// value it had, or null.
    pub fn hashmap_remove(&mut self, id: HeapId, key: &Value) -> VMResult<Value> {
        let key = self.existing_key(id, key);
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => {
                let old = map.shift_remove(&key);
                if old.is_some() {
                    self.forget_key(id, &key);
                }
                Ok(old.unwrap_or(Value::Reference(ObjectRef::Null)))
            }
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    pub fn hashmap_clear(&mut self, id: HeapId) -> VMResult<()> {
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => {
                map.clear();
                self.forget_keys(id);
                Ok(())
            }
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    /// Get HashMap size
    pub fn hashmap_size(&self, id: HeapId) -> VMResult<i32> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => Ok(map.len() as i32),
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    /// Check if HashMap contains key
    pub fn hashmap_contains_key(&self, id: HeapId, key: &Value) -> VMResult<bool> {
        let key = self.existing_key(id, key);
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashMap(map),
                ..
            }) => {
                // O(1) lookup
                Ok(map.contains_key(&key))
            }
            _ => Err(VMError::type_mismatch("HashMap", "other")),
        }
    }

    /// Add element to HashSet, returns true if element was added
    pub fn hashset_add(&mut self, id: HeapId, value: Value) -> VMResult<bool> {
        let value = self.canonical_key(id, &value);
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashSet(set),
                ..
            }) => {
                // O(1) insert - returns true if newly inserted
                Ok(set.insert(value))
            }
            _ => Err(VMError::type_mismatch("HashSet", "other")),
        }
    }

    /// Check if HashSet contains value
    pub fn hashset_contains(&self, id: HeapId, value: &Value) -> VMResult<bool> {
        let value = self.existing_key(id, value);
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashSet(set),
                ..
            }) => {
                // O(1) lookup
                Ok(set.contains(&value))
            }
            _ => Err(VMError::type_mismatch("HashSet", "other")),
        }
    }

    /// Remove value from a HashSet. Returns true if it was present.
    pub fn hashset_remove(&mut self, id: HeapId, value: &Value) -> VMResult<bool> {
        let value = self.existing_key(id, value);
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashSet(set),
                ..
            }) => {
                let removed = set.remove(&value);
                if removed {
                    self.forget_key(id, &value);
                }
                Ok(removed)
            }
            _ => Err(VMError::type_mismatch("HashSet", "other")),
        }
    }

    /// Get HashSet size
    pub fn hashset_size(&self, id: HeapId) -> VMResult<i32> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Collection(CollectionInstance {
                data: CollectionData::HashSet(set),
                ..
            }) => Ok(set.len() as i32),
            _ => Err(VMError::type_mismatch("HashSet", "other")),
        }
    }

    // === Thread Storage Methods ===

    /// Allocate a Thread on the heap with default name
    pub fn allocate_thread(&mut self) -> VMResult<HeapId> {
        static THREAD_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

        let thread_id = THREAD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = format!("Thread-{}", thread_id);

        self.allocate_entry(HeapEntry::Thread(ThreadData {
            thread_id,
            name,
            priority: 5, // NORM_PRIORITY
            is_daemon: false,
            state: ThreadState::New,
            target: ObjectRef::Null,
            interrupted: false,
        }))
    }

    /// Allocate a Thread on the heap with a Runnable target
    pub fn allocate_thread_with_runnable(&mut self, target: ObjectRef) -> VMResult<HeapId> {
        static THREAD_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

        let thread_id = THREAD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = format!("Thread-{}", thread_id);

        self.allocate_entry(HeapEntry::Thread(ThreadData {
            thread_id,
            name,
            priority: 5,
            is_daemon: false,
            state: ThreadState::New,
            target,
            interrupted: false,
        }))
    }

    /// Allocate a Thread on the heap with a name
    pub fn allocate_thread_with_name(&mut self, name: String) -> VMResult<HeapId> {
        static THREAD_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

        let thread_id = THREAD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        self.allocate_entry(HeapEntry::Thread(ThreadData {
            thread_id,
            name,
            priority: 5,
            is_daemon: false,
            state: ThreadState::New,
            target: ObjectRef::Null,
            interrupted: false,
        }))
    }

    /// Get Thread data by ID
    pub fn get_thread(&self, id: HeapId) -> VMResult<&ThreadData> {
        let entry = self.entry(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Thread(thread) => Ok(thread),
            _ => Err(VMError::type_mismatch("Thread", "other")),
        }
    }

    /// Get mutable Thread data by ID
    fn get_thread_mut(&mut self, id: HeapId) -> VMResult<&mut ThreadData> {
        let entry = self.entry_mut(id).ok_or_else(VMError::invalid_object_ref)?;

        match entry {
            HeapEntry::Thread(thread) => Ok(thread),
            _ => Err(VMError::type_mismatch("Thread", "other")),
        }
    }

    /// Set thread state
    pub fn set_thread_state(&mut self, id: HeapId, state: ThreadState) -> VMResult<()> {
        let thread = self.get_thread_mut(id)?;
        thread.state = state;
        Ok(())
    }

    /// Get thread name
    pub fn get_thread_name(&self, id: HeapId) -> VMResult<String> {
        let thread = self.get_thread(id)?;
        Ok(thread.name.clone())
    }

    /// Set thread name
    pub fn set_thread_name(&mut self, id: HeapId, name: String) -> VMResult<()> {
        let old_capacity = self.get_thread(id)?.name.capacity();
        let new_capacity = name.capacity();
        let new_memory_used = if new_capacity >= old_capacity {
            self.memory_after_allocation(new_capacity - old_capacity)?
        } else {
            self.memory_used
                .checked_sub(old_capacity - new_capacity)
                .expect("heap memory ledger underflow during thread rename")
        };
        let allocation = self
            .objects
            .get_mut(&id)
            .expect("validated thread disappeared during rename");
        let HeapEntry::Thread(thread) = &mut allocation.entry else {
            unreachable!("validated thread changed type during rename");
        };
        thread.name = name;
        allocation.charged_bytes = if new_capacity >= old_capacity {
            allocation
                .charged_bytes
                .checked_add(new_capacity - old_capacity)
                .expect("thread-name charge overflow")
        } else {
            allocation
                .charged_bytes
                .checked_sub(old_capacity - new_capacity)
                .expect("thread-name charge underflow")
        };
        self.memory_used = new_memory_used;
        self.debug_assert_accounting_consistent();
        Ok(())
    }

    /// Get thread ID (Java's Thread.getId())
    pub fn get_thread_id(&self, id: HeapId) -> VMResult<i64> {
        let thread = self.get_thread(id)?;
        Ok(thread.thread_id)
    }

    /// Check if thread is alive
    pub fn is_thread_alive(&self, id: HeapId) -> VMResult<bool> {
        let thread = self.get_thread(id)?;
        Ok(matches!(
            thread.state,
            ThreadState::Runnable
                | ThreadState::Blocked
                | ThreadState::Waiting
                | ThreadState::TimedWaiting
        ))
    }

    /// Interrupt a thread
    pub fn interrupt_thread(&mut self, id: HeapId) -> VMResult<()> {
        let thread = self.get_thread_mut(id)?;
        thread.interrupted = true;
        Ok(())
    }

    /// Check if thread is interrupted
    pub fn is_thread_interrupted(&self, id: HeapId) -> VMResult<bool> {
        let thread = self.get_thread(id)?;
        Ok(thread.interrupted)
    }

    /// Clear interrupted status and return previous value
    pub fn clear_thread_interrupted(&mut self, id: HeapId) -> VMResult<bool> {
        let thread = self.get_thread_mut(id)?;
        let was_interrupted = thread.interrupted;
        thread.interrupted = false;
        Ok(was_interrupted)
    }

    /// Get thread target (Runnable)
    pub fn get_thread_target(&self, id: HeapId) -> VMResult<ObjectRef> {
        let thread = self.get_thread(id)?;
        Ok(thread.target)
    }

    /// Set thread target (Runnable)
    pub fn set_thread_target(&mut self, id: HeapId, target: ObjectRef) -> VMResult<()> {
        let thread = self.get_thread_mut(id)?;
        thread.target = target;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::gc::{GCRoots, GarbageCollector};

    fn assert_accounting_consistent(heap: &Heap) {
        assert_eq!(
            heap.memory_used,
            heap.objects
                .values()
                .map(|allocation| allocation.charged_bytes)
                .sum::<usize>()
        );
    }

    #[test]
    fn throwable_sidecar_keeps_ordinary_object_layout_pointer_sized() {
        assert_eq!(
            std::mem::size_of::<Option<Box<ThrowableData>>>(),
            std::mem::size_of::<usize>()
        );
        assert!(std::mem::size_of::<ThrowableData>() <= 3 * std::mem::size_of::<usize>());
    }

    #[test]
    fn test_allocate_object() {
        let mut heap = Heap::new(1024 * 1024);
        let class = ClassRef {
            name: "Test".to_string(),
            instance_field_count: 2,
            static_field_count: 0,
        };

        let id = heap.allocate_object(class).unwrap();
        assert_ne!(id.0, 0);
        assert!(heap.memory_used() > 0);
        assert_accounting_consistent(&heap);
    }

    #[test]
    fn field_growth_is_charged_and_swept_from_the_allocation_ledger() {
        let mut heap = Heap::new(1024 * 1024);
        let live = heap
            .allocate_object(ClassRef {
                name: "Live".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })
            .unwrap();
        let dead = heap
            .allocate_object(ClassRef {
                name: "Dead".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })
            .unwrap();
        let before_growth = heap.memory_used;

        heap.set_field(dead, 7, Value::Int(42)).unwrap();

        assert!(heap.memory_used > before_growth);
        assert_eq!(heap.get_field(dead, 7).unwrap(), Some(Value::Int(42)));
        assert_accounting_consistent(&heap);

        let mut roots = GCRoots::new();
        roots.add_heap_id(live);
        let stats = GarbageCollector::collect(&mut heap, &roots);

        assert_eq!(stats.objects_freed, 1);
        assert!(heap.get_entry(live).is_some());
        assert!(heap.get_entry(dead).is_none());
        assert_accounting_consistent(&heap);
    }

    #[test]
    fn rejected_field_growth_is_failure_atomic() {
        let mut heap = Heap::new(1024 * 1024);
        let id = heap
            .allocate_object(ClassRef {
                name: "Dynamic".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })
            .unwrap();
        heap.memory_limit = heap.memory_used;
        let memory_used = heap.memory_used;
        let charge = heap.objects[&id].charged_bytes;

        assert!(matches!(
            heap.set_field(id, 0, Value::Int(42)),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert!(matches!(
            heap.set_field(id, usize::MAX, Value::Int(42)),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert!(heap.get_field(id, 0).is_err());
        assert_eq!(heap.memory_used, memory_used);
        assert_eq!(heap.objects[&id].charged_bytes, charge);
        assert_accounting_consistent(&heap);
    }

    #[test]
    fn replacing_an_existing_field_succeeds_at_the_heap_limit() {
        let mut heap = Heap::new(1024 * 1024);
        let id = heap
            .allocate_object(ClassRef {
                name: "Fixed".to_string(),
                instance_field_count: 1,
                static_field_count: 0,
            })
            .unwrap();
        heap.memory_limit = heap.memory_used;
        let memory_used = heap.memory_used;

        heap.put_field(id, 0, Value::Int(7)).unwrap();
        heap.set_field(id, 0, Value::Int(9)).unwrap();

        assert_eq!(heap.get_field(id, 0).unwrap(), Some(Value::Int(9)));
        assert_eq!(heap.memory_used, memory_used);
        assert_accounting_consistent(&heap);
    }

    #[test]
    fn thread_rename_updates_its_charge_atomically() {
        let mut heap = Heap::new(1024 * 1024);
        let id = heap.allocate_thread_with_name("a".to_string()).unwrap();
        let initial_memory = heap.memory_used;
        heap.memory_limit = initial_memory;

        assert!(matches!(
            heap.set_thread_name(id, String::with_capacity(128)),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert_eq!(heap.get_thread_name(id).unwrap(), "a");
        assert_eq!(heap.memory_used, initial_memory);

        let replacement = String::from("b");
        assert!(replacement.capacity() <= heap.get_thread(id).unwrap().name.capacity());
        heap.set_thread_name(id, replacement).unwrap();
        assert_eq!(heap.get_thread_name(id).unwrap(), "b");
        assert_accounting_consistent(&heap);
    }

    #[test]
    fn rejected_object_and_throwable_layouts_are_atomic() {
        let mut heap = Heap::new(64);
        let object_count = heap.objects.len();
        let next_id = heap.next_id;
        let enormous = ClassRef {
            name: "Enormous".to_string(),
            instance_field_count: usize::MAX,
            static_field_count: 0,
        };

        assert!(matches!(
            heap.allocate_object(enormous.clone()),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert!(matches!(
            heap.allocate_throwable(enormous, ObjectRef::Null, None),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert_eq!(heap.memory_used, 0);
        assert_eq!(heap.objects.len(), object_count);
        assert_eq!(heap.next_id, next_id);
    }

    #[test]
    fn test_allocate_array() {
        let mut heap = Heap::new(1024 * 1024);

        let id = heap.allocate_array(ArrayElementType::Int, 10).unwrap();
        let len = heap.array_length(id).unwrap();
        assert_eq!(len, 10);
    }

    #[test]
    fn test_array_store_load() {
        let mut heap = Heap::new(1024 * 1024);
        let id = heap.allocate_array(ArrayElementType::Int, 5).unwrap();

        heap.array_store(id, 0, Value::Int(42)).unwrap();
        let value = heap.array_load(id, 0).unwrap();
        assert_eq!(value.as_int().unwrap(), 42);
    }

    #[test]
    fn test_array_out_of_bounds() {
        let mut heap = Heap::new(1024 * 1024);
        let id = heap.allocate_array(ArrayElementType::Int, 5).unwrap();

        assert!(heap.array_store(id, 5, Value::Int(42)).is_err());
        assert!(heap.array_load(id, 10).is_err());
    }

    #[test]
    fn test_negative_array_size() {
        let mut heap = Heap::new(1024 * 1024);
        assert!(heap.allocate_array(ArrayElementType::Int, -1).is_err());
    }

    #[test]
    fn test_max_array_size_enforced() {
        let mut heap = Heap::new(1024 * 1024 * 1024); // 1GB heap

        // Array size exactly at the limit should succeed (if we have enough memory)
        // Note: We can't actually test MAX_ARRAY_SIZE due to memory constraints,
        // but we can test that exceeding it fails

        // Try to allocate array larger than MAX_ARRAY_SIZE
        let too_large = i32::MAX - 7; // Just below MAX_ARRAY_SIZE
        let result = heap.allocate_array(ArrayElementType::Byte, too_large);
        // This should fail due to MAX_ARRAY_SIZE check
        assert!(
            result.is_err(),
            "Arrays larger than MAX_ARRAY_SIZE should be rejected"
        );

        // Verify MAX_ARRAY_SIZE + 1 is rejected
        let over_limit = MAX_ARRAY_SIZE + 1;
        let result = heap.allocate_array(ArrayElementType::Byte, over_limit);
        assert!(
            result.is_err(),
            "Arrays exceeding MAX_ARRAY_SIZE should be rejected"
        );
    }

    #[test]
    fn test_array_allocation_prevents_dos() {
        let mut heap = Heap::new(1024 * 1024 * 1024); // 1GB heap

        // Simulate malicious bytecode trying to allocate massive array
        // new int[Integer.MAX_VALUE] would be 8.6GB on 64-bit system
        let malicious_size = i32::MAX;
        let result = heap.allocate_array(ArrayElementType::Int, malicious_size);

        // Should be rejected immediately, not attempt allocation
        assert!(
            result.is_err(),
            "DoS via massive array allocation should be prevented"
        );
    }

    #[test]
    fn rejected_array_allocation_does_not_mutate_heap_state() {
        let mut heap = Heap::new(64);
        let object_count = heap.objects.len();
        let next_id = heap.next_id;

        assert!(matches!(
            heap.allocate_array(ArrayElementType::Int, 1_000_000),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert_eq!(heap.memory_used, 0);
        assert_eq!(heap.objects.len(), object_count);
        assert_eq!(heap.next_id, next_id);
    }

    #[test]
    fn exhausted_heap_ids_are_rejected_before_array_materialization() {
        let mut heap = Heap::new(16 * 1024 * 1024);
        heap.next_id = u32::MAX;
        let object_count = heap.objects.len();

        assert!(matches!(
            heap.allocate_array(ArrayElementType::Int, 1_000_000),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert_eq!(heap.memory_used, 0);
        assert_eq!(heap.objects.len(), object_count);
        assert_eq!(heap.next_id, u32::MAX);
    }

    #[test]
    fn rejected_array_clone_does_not_mutate_heap_state() {
        let mut heap = Heap::new(5_000);
        let source = heap.allocate_array(ArrayElementType::Int, 1_000).unwrap();
        let memory_used = heap.memory_used;
        let object_count = heap.objects.len();
        let next_id = heap.next_id;

        assert!(matches!(
            heap.clone_array(source),
            Err(VMError::MemoryLimitExceeded)
        ));
        assert_eq!(heap.memory_used, memory_used);
        assert_eq!(heap.objects.len(), object_count);
        assert_eq!(heap.next_id, next_id);
        assert_eq!(heap.array_length(source).unwrap(), 1_000);
    }

    #[test]
    fn gc_threshold_comparison_cannot_overflow() {
        let mut heap = Heap::new(10);
        heap.memory_used = 7;
        assert!(!heap.should_collect());
        heap.memory_used = 8;
        assert!(heap.should_collect());

        let mut maximal = Heap::new(usize::MAX);
        maximal.memory_used = ((usize::MAX as u128) * 7 / 10) as usize;
        assert!(!maximal.should_collect());
        maximal.memory_used += 1;
        assert!(maximal.should_collect());

        assert!(!Heap::new(0).should_collect());
    }

    #[test]
    fn test_string_interning() {
        let mut heap = Heap::new(1024 * 1024);

        let s1 = heap.intern_string("hello").unwrap();
        let s2 = heap.intern_string("hello").unwrap();

        assert_eq!(s1, s2);
    }

    #[test]
    fn test_get_string_reads_contents_by_id() {
        let mut heap = Heap::new(1024 * 1024);
        let ObjectRef::Object(alpha) = heap.intern_string("alpha").unwrap() else {
            panic!("expected an object reference");
        };
        let ObjectRef::Object(beta) = heap.intern_string("beta").unwrap() else {
            panic!("expected an object reference");
        };

        assert_eq!(heap.get_string(alpha).unwrap(), "alpha");
        assert_eq!(heap.get_string(beta).unwrap(), "beta");
    }

    #[test]
    fn test_array_class_name_is_its_type_descriptor() {
        let mut heap = Heap::new(1024 * 1024);
        let ints = heap.allocate_array(ArrayElementType::Int, 3).unwrap();
        let strings = heap
            .allocate_array(
                ArrayElementType::Reference("java/lang/String".to_string()),
                2,
            )
            .unwrap();

        assert_eq!(heap.get_object_class_name(ints).unwrap(), "[I");
        assert_eq!(
            heap.get_object_class_name(strings).unwrap(),
            "[Ljava/lang/String;"
        );
    }

    #[test]
    fn test_hashset_o1_operations() {
        let mut heap = Heap::new(1024 * 1024);
        let set_id = heap.allocate_hashset().unwrap();

        // Test O(1) insert
        assert!(heap.hashset_add(set_id, Value::Int(1)).unwrap());
        assert!(heap.hashset_add(set_id, Value::Int(2)).unwrap());
        assert!(!heap.hashset_add(set_id, Value::Int(1)).unwrap()); // Duplicate

        // Test O(1) lookup
        assert!(heap.hashset_contains(set_id, &Value::Int(1)).unwrap());
        assert!(heap.hashset_contains(set_id, &Value::Int(2)).unwrap());
        assert!(!heap.hashset_contains(set_id, &Value::Int(3)).unwrap());

        // Test size
        assert_eq!(heap.hashset_size(set_id).unwrap(), 2);

        // Test with different value types
        assert!(heap.hashset_add(set_id, Value::Long(100)).unwrap());
        assert!(heap.hashset_add(set_id, Value::Float(3.5)).unwrap());
        assert!(heap.hashset_add(set_id, Value::Double(2.75)).unwrap());

        assert_eq!(heap.hashset_size(set_id).unwrap(), 5);
        assert!(heap.hashset_contains(set_id, &Value::Float(3.5)).unwrap());
    }

    #[test]
    fn natural_order_preserves_long_precision_and_java_float_order() {
        let mut heap = Heap::new(1024 * 1024);
        let low = Value::Reference(ObjectRef::Object(
            heap.allocate_long(9_007_199_254_740_992).unwrap(),
        ));
        let high = Value::Reference(ObjectRef::Object(
            heap.allocate_long(9_007_199_254_740_993).unwrap(),
        ));
        assert_eq!(natural_cmp(&heap, &low, &high), std::cmp::Ordering::Less);

        let set = heap.allocate_treeset().unwrap();
        heap.hashset_add(set, high.clone()).unwrap();
        heap.hashset_add(set, low.clone()).unwrap();
        assert_eq!(heap.collection_snapshot(set).unwrap(), vec![low, high]);

        let negative_zero =
            Value::Reference(ObjectRef::Object(heap.allocate_double(-0.0).unwrap()));
        let positive_zero = Value::Reference(ObjectRef::Object(heap.allocate_double(0.0).unwrap()));
        let nan = Value::Reference(ObjectRef::Object(heap.allocate_double(f64::NAN).unwrap()));
        let positive_infinity = Value::Reference(ObjectRef::Object(
            heap.allocate_double(f64::INFINITY).unwrap(),
        ));
        assert_eq!(
            natural_cmp(&heap, &negative_zero, &positive_zero),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            natural_cmp(&heap, &positive_infinity, &nan),
            std::cmp::Ordering::Less
        );
        assert_eq!(natural_cmp(&heap, &nan, &nan), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_hashset_performance_no_quadratic() {
        let mut heap = Heap::new(10 * 1024 * 1024);
        let set_id = heap.allocate_hashset().unwrap();

        // Add 1000 elements - should be O(n) not O(n^2)
        for i in 0..1000 {
            assert!(heap.hashset_add(set_id, Value::Int(i)).unwrap());
        }

        // Verify all elements present - should be O(n) not O(n^2)
        for i in 0..1000 {
            assert!(heap.hashset_contains(set_id, &Value::Int(i)).unwrap());
        }

        assert_eq!(heap.hashset_size(set_id).unwrap(), 1000);
    }

    #[test]
    fn int_range_iterator_is_lazy_and_honors_direction_and_bounds() {
        let mut heap = Heap::new(1024 * 1024);

        let ascending = heap.allocate_int_range_iterator(1, 3, true).unwrap();
        for expected in 1..=3 {
            assert!(heap.iterator_has_next(ascending).unwrap());
            let Value::Reference(ObjectRef::Object(id)) = heap.iterator_next(ascending).unwrap()
            else {
                panic!("IntRange.next must return a boxed Integer");
            };
            assert_eq!(heap.get_boxed_int(id).unwrap(), expected);
        }
        assert!(!heap.iterator_has_next(ascending).unwrap());

        let descending = heap.allocate_int_range_iterator(5, 1, false).unwrap();
        let mut values = Vec::new();
        while heap.iterator_has_next(descending).unwrap() {
            let Value::Reference(ObjectRef::Object(id)) = heap.iterator_next(descending).unwrap()
            else {
                panic!("IntRange.next must return a boxed Integer");
            };
            values.push(heap.get_boxed_int(id).unwrap());
        }
        assert_eq!(values, vec![5, 4, 3, 2]);

        let boundary = heap
            .allocate_int_range_iterator(i32::MAX - 1, i32::MAX, true)
            .unwrap();
        assert!(heap.iterator_has_next(boundary).unwrap());
        let _ = heap.iterator_next(boundary).unwrap();
        assert!(heap.iterator_has_next(boundary).unwrap());
        let _ = heap.iterator_next(boundary).unwrap();
        assert!(!heap.iterator_has_next(boundary).unwrap());
    }
}
