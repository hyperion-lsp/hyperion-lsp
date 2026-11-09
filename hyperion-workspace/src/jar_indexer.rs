// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JAR file indexing support
//!
//! This module provides functionality to index Java classes from JAR files,
//! extracting class names, methods, and fields to enable navigation into
//! external dependencies.
//!
//! Uses hyperion-bytecode to extract parameter names from LocalVariableTable
//! attribute when available (requires -parameters compilation flag).

use anyhow::{Context, Result};
use hyperion_bytecode::descriptor::DescriptorType;
use hyperion_bytecode::extractor::{
    extract_generic_supertypes, extract_symbols, is_static_member_class, ClassSymbols, FieldSymbol,
    MethodSymbol,
};
use hyperion_bytecode::kotlin_metadata::{
    KotlinClassInfo, KotlinClassMetadata, KotlinClassifier, KotlinConstructorInfo,
    KotlinFunctionInfo, KotlinJvmMethodSignature, KotlinMetadataKind, KotlinParameterInfo,
    KotlinPropertyInfo, KotlinTypeArgument, KotlinTypeInfo, KotlinTypeParameterInfo,
    KotlinVariance,
};
use hyperion_bytecode::parser::{
    binary_reference_to_source_name, get_source_class_name, parse_class_file as parse_bytecode,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};
use zip::ZipArchive;

/// Maximum size for a single class file (16 MiB).
/// Prevents ZIP bomb attacks where a small compressed file expands to gigabytes.
const MAX_CLASS_FILE_SIZE: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_ENTRIES: usize = 250_000;
const MAX_CLASS_ENTRIES: usize = 100_000;
const MAX_TOTAL_CLASS_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_SYMBOLS: usize = 500_000;

use crate::javadoc_parser::parse_javadoc_jar;
use crate::{
    FileId, KotlinParameterMetadata, KotlinPropertyMetadata, KotlinSymbolMetadata, Modifiers,
    Symbol, TextRange, TypeParameterInfo,
};
use lsp_types::SymbolKind;

#[derive(Debug, Default)]
pub(crate) struct ArchiveIndexData {
    pub symbols: Vec<Symbol>,
    pub generic_supertypes: Vec<crate::TypeGenericSupertypes>,
}

#[derive(Debug, Default)]
pub(crate) struct ClassIndexData {
    pub symbols: Vec<Symbol>,
    pub generic_supertypes: Vec<crate::TypeGenericSupertypes>,
}

/// Extract simple name from FQN
///
/// Examples:
/// - "java.lang.String" → "String"
/// - "com.example.User" → "User"
fn extract_simple_name(fqn: &str) -> String {
    fqn.rsplit('.').next().unwrap_or(fqn).to_string()
}

/// Convert a JVM descriptor to its canonical source-level type identity.
/// Both symbol keys and semantic member types retain qualification; shortening
/// belongs in presentation code, after overload resolution and type checking.
fn descriptor_to_source_type(desc: &DescriptorType) -> String {
    match desc {
        DescriptorType::Byte => "byte".to_string(),
        DescriptorType::Char => "char".to_string(),
        DescriptorType::Double => "double".to_string(),
        DescriptorType::Float => "float".to_string(),
        DescriptorType::Int => "int".to_string(),
        DescriptorType::Long => "long".to_string(),
        DescriptorType::Short => "short".to_string(),
        DescriptorType::Boolean => "boolean".to_string(),
        DescriptorType::Void => "void".to_string(),
        DescriptorType::Class { name } => binary_reference_to_source_name(name).into_owned(),
        DescriptorType::Array {
            element_type,
            dimensions,
        } => {
            let mut base = descriptor_to_source_type(element_type);
            for _ in 0..*dimensions {
                base.push_str("[]");
            }
            base
        }
    }
}

/// Statistics from indexing a JAR file
#[derive(Debug, Default)]
pub struct JarIndexStats {
    pub classes_found: usize,
    pub classes_indexed: usize,
    /// Archive entries that were not indexed: a class entry that was oversized, unreadable
    /// or unparseable, and an entry whose header could not be read. The rest of the
    /// archive is still indexed.
    pub entries_skipped: usize,
    pub methods_indexed: usize,
    pub fields_indexed: usize,
    /// True only when a requested Javadoc attachment was parsed and merged.
    pub documentation_loaded: bool,
    /// Parse/open error for an optional requested Javadoc attachment.
    pub documentation_error: Option<String>,
}

impl JarIndexStats {
    /// Use the same member categories for JAR and JMOD extraction. Constructors
    /// are callable symbols too; omitting them undercounts extracted members
    /// even before the index deduplicates symbol keys.
    pub(crate) fn record_class_symbols(&mut self, symbols: &[Symbol]) {
        self.classes_indexed += 1;
        for symbol in symbols {
            match symbol.kind {
                SymbolKind::METHOD | SymbolKind::FUNCTION | SymbolKind::CONSTRUCTOR => {
                    self.methods_indexed += 1;
                }
                SymbolKind::FIELD | SymbolKind::PROPERTY => self.fields_indexed += 1,
                _ => {}
            }
        }
    }
}

/// Cumulative work/output budget for one untrusted bytecode archive.
pub(crate) struct ArchiveWorkBudget {
    archive_kind: &'static str,
    classes: usize,
    expanded_bytes: u64,
    symbols: usize,
    max_classes: usize,
    max_expanded_bytes: u64,
    max_symbols: usize,
}

impl ArchiveWorkBudget {
    pub(crate) const fn standard(archive_kind: &'static str) -> Self {
        Self {
            archive_kind,
            classes: 0,
            expanded_bytes: 0,
            symbols: 0,
            max_classes: MAX_CLASS_ENTRIES,
            max_expanded_bytes: MAX_TOTAL_CLASS_BYTES,
            max_symbols: MAX_TOTAL_SYMBOLS,
        }
    }

    #[cfg(test)]
    const fn with_limits(
        archive_kind: &'static str,
        max_classes: usize,
        max_expanded_bytes: u64,
        max_symbols: usize,
    ) -> Self {
        Self {
            archive_kind,
            classes: 0,
            expanded_bytes: 0,
            symbols: 0,
            max_classes,
            max_expanded_bytes,
            max_symbols,
        }
    }

    pub(crate) fn record_class(&mut self, expanded_bytes: u64) -> Result<()> {
        self.classes = self
            .classes
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("class entry count overflowed"))?;
        if self.classes > self.max_classes {
            anyhow::bail!(
                "{} contains more than {} class entries",
                self.archive_kind,
                self.max_classes
            );
        }
        self.expanded_bytes = self
            .expanded_bytes
            .checked_add(expanded_bytes)
            .ok_or_else(|| anyhow::anyhow!("expanded class byte count overflowed"))?;
        if self.expanded_bytes > self.max_expanded_bytes {
            anyhow::bail!(
                "{} class data expands beyond {} total bytes",
                self.archive_kind,
                self.max_expanded_bytes
            );
        }
        Ok(())
    }

    pub(crate) fn record_symbols(&mut self, symbols: usize) -> Result<()> {
        self.symbols = self
            .symbols
            .checked_add(symbols)
            .ok_or_else(|| anyhow::anyhow!("symbol count overflowed"))?;
        if self.symbols > self.max_symbols {
            anyhow::bail!(
                "{} contains more than {} symbols",
                self.archive_kind,
                self.max_symbols
            );
        }
        Ok(())
    }
}

/// Extract symbols from a JAR file
///
/// Returns a vector of symbols (classes, methods, fields) found in the JAR,
/// along with statistics about what was indexed.
pub fn extract_jar_symbols(
    jar_path: &Path,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jar_index_data(jar_path, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jar_index_data(
    jar_path: &Path,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let file = File::open(jar_path)
        .with_context(|| format!("Failed to open JAR file: {}", jar_path.display()))?;

    extract_jar_index_data_from_file(file, jar_path, file_id)
}

/// Extract symbols from an immutable private archive snapshot.
pub fn extract_jar_symbols_from_snapshot(
    snapshot: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jar_index_data_from_snapshot(snapshot, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jar_index_data_from_snapshot(
    snapshot: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let display_path = snapshot.display_path().to_path_buf();
    extract_jar_index_data_from_file(snapshot.into_file(), &display_path, file_id)
}

fn extract_jar_index_data_from_file(
    file: File,
    jar_path: &Path,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read JAR archive: {}", jar_path.display()))?;

    let mut data = ArchiveIndexData::default();
    let mut stats = JarIndexStats::default();
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        anyhow::bail!(
            "JAR contains {} entries (maximum {})",
            archive.len(),
            MAX_ARCHIVE_ENTRIES
        );
    }
    let mut budget = ArchiveWorkBudget::standard("JAR");

    for i in 0..archive.len() {
        // One unreadable entry header must not discard the rest of the archive
        let file = match archive.by_index(i) {
            Ok(file) => file,
            Err(e) => {
                warn!(
                    "Skipping unreadable entry {i} in {}: {e}",
                    jar_path.display()
                );
                stats.entries_skipped += 1;
                continue;
            }
        };
        let name = match file.name() {
            Ok(name) => name.into_owned(),
            Err(e) => {
                warn!("Skipping invalid entry name in {}: {e}", jar_path.display());
                stats.entries_skipped += 1;
                continue;
            }
        };

        // META-INF classes (including multi-release overlays) are not direct
        // classpath entries and would otherwise duplicate base symbols.
        if !is_indexable_class_entry(&name) {
            continue;
        }

        stats.classes_found += 1;

        // Check file size before reading (ZIP bomb protection)
        let uncompressed_size = file.size();
        if uncompressed_size > MAX_CLASS_FILE_SIZE {
            warn!(
                "Skipping oversized class file {} ({} bytes > {} max)",
                name, uncompressed_size, MAX_CLASS_FILE_SIZE
            );
            stats.entries_skipped += 1;
            continue;
        }

        // Read class file bytes with size limit
        let mut bytes = Vec::with_capacity(uncompressed_size as usize);
        if let Err(e) = file.take(MAX_CLASS_FILE_SIZE + 1).read_to_end(&mut bytes) {
            warn!("Failed to read class file {}: {}", name, e);
            stats.entries_skipped += 1;
            continue;
        }
        if bytes.len() as u64 > MAX_CLASS_FILE_SIZE {
            warn!(
                "Skipping class file {} that expanded beyond the size limit",
                name
            );
            stats.entries_skipped += 1;
            continue;
        }
        budget.record_class(bytes.len() as u64)?;

        // Parse class file
        match parse_class_file_data(&bytes, &name, file_id) {
            Ok(mut class_data) => {
                budget.record_symbols(class_data.symbols.len())?;
                stats.record_class_symbols(&class_data.symbols);

                data.symbols.append(&mut class_data.symbols);
                data.generic_supertypes
                    .append(&mut class_data.generic_supertypes);
            }
            Err(e) => {
                debug!("Failed to parse class file {}: {}", name, e);
                stats.entries_skipped += 1;
                continue;
            }
        }
    }

    debug!(
        "Indexed JAR {}: {} classes, {} methods, {} fields",
        jar_path.display(),
        stats.classes_indexed,
        stats.methods_indexed,
        stats.fields_indexed
    );

    Ok((data, stats))
}

fn is_indexable_class_entry(name: &str) -> bool {
    name.ends_with(".class")
        && !name.starts_with("META-INF/")
        && name != "module-info.class"
        && !name.ends_with("/module-info.class")
        && name != "package-info.class"
        && !name.ends_with("/package-info.class")
}

/// Extract symbols from a JAR file with documentation from a javadoc JAR
///
/// This function indexes both the main JAR and the javadoc JAR, merging
/// documentation into the extracted symbols.
pub fn extract_jar_symbols_with_docs(
    jar_path: &Path,
    javadoc_jar_path: &Path,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jar_index_data_with_docs(jar_path, javadoc_jar_path, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jar_index_data_with_docs(
    jar_path: &Path,
    javadoc_jar_path: &Path,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    // First, extract javadoc documentation
    let javadoc = parse_javadoc_jar(javadoc_jar_path);

    // Then extract symbols from the main JAR
    let (mut data, mut stats) = extract_jar_index_data(jar_path, file_id)?;

    // Merge documentation into symbols
    match javadoc {
        Ok(javadoc) => {
            stats.documentation_loaded = true;
            merge_javadoc(&mut data.symbols, &javadoc);

            debug!(
                "Merged documentation: {} classes, {} methods, {} fields have docs",
                data.symbols
                    .iter()
                    .filter(|s| matches!(
                        s.kind,
                        SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM
                    ) && s.documentation.is_some())
                    .count(),
                data.symbols
                    .iter()
                    .filter(|s| s.kind == SymbolKind::METHOD && s.documentation.is_some())
                    .count(),
                data.symbols
                    .iter()
                    .filter(|s| s.kind == SymbolKind::FIELD && s.documentation.is_some())
                    .count()
            );
        }
        Err(error) => {
            warn!("Failed to parse javadoc JAR: {error}");
            stats.documentation_error = Some(error);
        }
    }

    Ok((data, stats))
}

/// Extract symbols and optional documentation from immutable snapshots.
pub fn extract_jar_symbols_with_docs_from_snapshots(
    jar: crate::ArchiveSnapshot,
    javadoc: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jar_index_data_with_docs_from_snapshots(jar, javadoc, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jar_index_data_with_docs_from_snapshots(
    jar: crate::ArchiveSnapshot,
    javadoc: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let (mut data, mut stats) = extract_jar_index_data_from_snapshot(jar, file_id)?;
    match crate::javadoc_parser::parse_javadoc_snapshot(javadoc) {
        Ok(javadoc) => {
            merge_javadoc(&mut data.symbols, &javadoc);
            stats.documentation_loaded = true;
        }
        Err(error) => stats.documentation_error = Some(error),
    }
    Ok((data, stats))
}

fn merge_javadoc(symbols: &mut [Symbol], javadoc: &crate::JavadocExtraction) {
    for symbol in symbols {
        match symbol.kind {
            SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM => {
                if let Some(doc) = javadoc.class_docs.get(symbol.fqn.as_ref()) {
                    symbol.documentation = Some(Arc::from(doc.as_str()));
                }
            }
            SymbolKind::METHOD => {
                // Javadoc is keyed by name and arity, as the anchor determines them
                if let Some(container) = symbol.container.as_deref() {
                    let key = format!(
                        "{container}.{}/{}",
                        symbol.simple_name,
                        symbol.parameters.len()
                    );
                    if let Some(doc) = javadoc.method_docs.get(&key) {
                        symbol.documentation = Some(Arc::from(doc.as_str()));
                    }
                }
            }
            SymbolKind::FIELD => {
                if let Some(doc) = javadoc.field_docs.get(symbol.fqn.as_ref()) {
                    symbol.documentation = Some(Arc::from(doc.as_str()));
                }
            }
            _ => {}
        }
    }
}

/// Parse a single class file and extract symbols using hyperion-bytecode
///
/// This extracts parameter names from LocalVariableTable attribute when available.
/// External JARs need -parameters compilation flag for real parameter names.
pub(crate) fn parse_class_file_data(
    bytes: &[u8],
    _class_path: &str,
    _file_id: FileId,
) -> Result<ClassIndexData> {
    // Parse bytecode
    let class_file = parse_bytecode(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse class file: {:?}", e))?;

    // Read language metadata before converting the bytecode identity. Kotlin's
    // metadata carries its authoritative source-level nested class name.
    let kotlin_info = KotlinClassMetadata::read(&class_file);
    let Some(mut source_fqn) = get_source_class_name(&class_file) else {
        // Anonymous and local classes have no importable workspace-wide source
        // name. Their bytecode remains available to the VM, but they do not
        // become external index symbols.
        return Ok(ClassIndexData::default());
    };
    if let Some(info) = &kotlin_info {
        if info.metadata.kind == KotlinMetadataKind::Class {
            source_fqn.clone_from(&info.name);
        }
    }

    // Extract symbols with parameter names
    let generic_supertypes = extract_generic_supertypes(&class_file);
    let is_static_member = is_static_member_class(&class_file);
    let class_symbols = extract_symbols(&class_file)
        .map_err(|e| anyhow::anyhow!("Failed to extract symbols: {:?}", e))?;

    // Prefer Kotlin's source-level declarations over their lowered JVM shape.
    // Kind-4 multifile facades: their DECLARATIONS live in the kind-5 part
    // classes and must not be indexed twice — but the facade class itself
    // (`CollectionsKt`, `StringsKt`) is the Java-importable entry point, so
    // its class identity must be indexed. Dropping it entirely made
    // `import kotlin.collections.CollectionsKt;` in a Java file a false
    // "undefined symbol" while sibling class imports worked.
    let metadata_type_fqn = source_fqn.clone();
    let mut symbols = match kotlin_info {
        Some(info) if info.metadata.kind == KotlinMetadataKind::MultiFileClassFacade => {
            let mut indexed_class_symbols = class_symbols;
            normalize_class_symbols_for_index(&mut indexed_class_symbols, source_fqn);
            vec![convert_class_symbol(&indexed_class_symbols)]
        }
        Some(info)
            if matches!(
                info.metadata.kind,
                KotlinMetadataKind::Class
                    | KotlinMetadataKind::FileFacade
                    | KotlinMetadataKind::MultiFileClassPart
            ) =>
        {
            // Kotlin metadata/JVM-signature matching must retain raw `$`
            // descriptors, while emitted workspace symbols use source names.
            let mut indexed_class_symbols = class_symbols.clone();
            normalize_class_symbols_for_index(&mut indexed_class_symbols, source_fqn);
            convert_kotlin_class_symbols(&class_symbols, &indexed_class_symbols, &info)
        }
        _ => {
            let mut indexed_class_symbols = class_symbols;
            normalize_class_symbols_for_index(&mut indexed_class_symbols, source_fqn);
            convert_java_class_symbols(&indexed_class_symbols)
        }
    };
    if is_static_member {
        if let Some(declaration) = symbols
            .iter_mut()
            .find(|symbol| symbol.fqn.as_ref() == metadata_type_fqn)
        {
            declaration.modifiers.is_static = true;
        }
    }
    let generic_supertypes = if generic_supertypes.is_empty() {
        Vec::new()
    } else {
        vec![crate::TypeGenericSupertypes {
            type_fqn: metadata_type_fqn,
            edges: generic_supertypes
                .into_iter()
                .map(|edge| crate::GenericSupertype {
                    written_target: edge.target,
                    arguments: edge.arguments,
                })
                .collect(),
        }]
    };
    Ok(ClassIndexData {
        symbols,
        generic_supertypes,
    })
}

pub(crate) fn normalize_class_symbols_for_index(class: &mut ClassSymbols, source_fqn: String) {
    class.fqn = source_fqn;
    class.super_class = class
        .super_class
        .take()
        .map(|name| binary_reference_to_source_name(&name).into_owned());
    class.interfaces = class
        .interfaces
        .drain(..)
        .map(|name| binary_reference_to_source_name(&name).into_owned())
        .collect();
    for method in &mut class.methods {
        for parameter in &mut method.parameters {
            normalize_descriptor_type(parameter);
        }
        normalize_descriptor_type(&mut method.return_type);
    }
    for field in &mut class.fields {
        normalize_descriptor_type(&mut field.field_type);
    }
}

fn normalize_descriptor_type(ty: &mut DescriptorType) {
    match ty {
        DescriptorType::Class { name } => {
            *name = binary_reference_to_source_name(name).into_owned();
        }
        DescriptorType::Array { element_type, .. } => normalize_descriptor_type(element_type),
        _ => {}
    }
}

fn convert_kotlin_class_symbols(
    class: &ClassSymbols,
    indexed_class: &ClassSymbols,
    info: &KotlinClassInfo,
) -> Vec<Symbol> {
    let is_package = matches!(
        info.metadata.kind,
        KotlinMetadataKind::FileFacade | KotlinMetadataKind::MultiFileClassPart
    );
    let declaration_container = if info.metadata.kind == KotlinMetadataKind::MultiFileClassPart {
        info.metadata
            .extra_string
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(|name| name.replace('/', "."))
            .unwrap_or_else(|| indexed_class.fqn.clone())
    } else {
        indexed_class.fqn.clone()
    };
    let mut symbols = Vec::new();
    let mut claimed_methods = HashSet::new();
    let mut claimed_fields = HashSet::new();

    if !is_package {
        let mut class_symbol = convert_class_symbol(indexed_class);
        class_symbol.modifiers.is_data = info.is_data_class;
        class_symbol.modifiers.is_sealed = info.is_sealed;
        class_symbol.type_parameters = kotlin_type_parameters(&info.type_parameters);
        symbols.push(class_symbol);
    } else if info.metadata.kind == KotlinMetadataKind::FileFacade {
        // A file facade (`UtilsKt`) is itself a Java-importable class holding
        // the file's top-level functions as statics — index its class
        // identity so `import com.lib.UtilsKt;` resolves. Its functions are
        // indexed below with the facade as container. Kind-5 multifile parts
        // stay suppressed: their public part-class names
        // (`CollectionsKt__CollectionsJVMKt`) are compiler artifacts and
        // their declarations route to the kind-4 facade.
        let mut facade = convert_class_symbol(indexed_class);
        let type_aliases = kotlin_package_type_aliases(&indexed_class.fqn, info);
        if !type_aliases.is_empty() {
            facade.kotlin = Some(Box::new(KotlinSymbolMetadata {
                type_aliases,
                ..KotlinSymbolMetadata::default()
            }));
        }
        symbols.push(facade);
    }

    for function in &info.functions {
        let method_index = find_function_method(class, info, function);
        if let Some(index) = method_index {
            claimed_methods.insert(index);
        }
        symbols.push(convert_kotlin_function(
            function,
            method_index.map(|index| &class.methods[index]),
            &declaration_container,
            is_package,
        ));
    }

    for constructor in &info.constructors {
        let method_index = find_constructor_method(class, info, constructor);
        if let Some(index) = method_index {
            claimed_methods.insert(index);
        }
        symbols.push(convert_kotlin_constructor(
            constructor,
            method_index.map(|index| &class.methods[index]),
            &indexed_class.fqn,
        ));
    }

    for property in &info.properties {
        let matches = find_property_members(class, info, property);
        claimed_methods.extend(matches.methods.iter().copied());
        if let Some(index) = matches.field {
            claimed_fields.insert(index);
        }
        symbols.push(convert_kotlin_property(
            property,
            matches.methods.first().map(|index| &class.methods[*index]),
            matches.field.map(|index| &class.fields[index]),
            &declaration_container,
            is_package,
        ));
    }

    // Preserve JVM-visible declarations that have no Kotlin metadata entry
    // (for example enum helpers and explicit @JvmStatic bridges), while
    // suppressing lowered implementations of declarations already emitted.
    for (index, method) in class.methods.iter().enumerate() {
        if claimed_methods.contains(&index)
            || method.is_constructor()
            || is_kotlin_lowering_method(method, info)
        {
            continue;
        }
        if let Some(symbol) =
            convert_method_symbol(&indexed_class.methods[index], &indexed_class.fqn)
        {
            symbols.push(symbol);
        }
    }
    for (index, field) in class.fields.iter().enumerate() {
        if claimed_fields.contains(&index)
            || info
                .properties
                .iter()
                .any(|property| property.name == field.name)
            || field.name == "INSTANCE"
        {
            continue;
        }
        if let Some(symbol) = convert_field_symbol(&indexed_class.fields[index], &indexed_class.fqn)
        {
            symbols.push(symbol);
        }
    }

    symbols
}

fn convert_class_symbol(class: &ClassSymbols) -> Symbol {
    let type_parameters = class
        .type_parameters
        .iter()
        .enumerate()
        .map(|(i, name)| TypeParameterInfo::new(name.clone(), i))
        .collect();
    Symbol {
        fqn: class.fqn.clone().into(),
        simple_name: extract_simple_name(&class.fqn).into(),
        range: TextRange::new(0, 0),
        kind: if class.is_interface {
            SymbolKind::INTERFACE
        } else {
            SymbolKind::CLASS
        },
        modifiers: Modifiers {
            is_public: class.is_public,
            is_final: class.is_final,
            is_abstract: class.is_abstract,
            ..Modifiers::default()
        },
        container: None,
        return_type: None,
        parameters: Vec::new(),
        parent_class: class.super_class.clone().map(Arc::from),
        interfaces: class
            .interfaces
            .iter()
            .map(|name| Arc::from(name.as_str()))
            .collect(),
        documentation: None,
        type_parameters,
        is_deprecated: class.is_deprecated,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

fn convert_kotlin_function(
    function: &KotlinFunctionInfo,
    method: Option<&MethodSymbol>,
    class_fqn: &str,
    is_package: bool,
) -> Symbol {
    let parameters: Vec<Arc<str>> = function
        .parameters
        .iter()
        .map(kotlin_parameter_display)
        .map(Arc::from)
        .collect();
    let fqn_parameters = function
        .parameters
        .iter()
        .map(|parameter| kotlin_type_display(&parameter.ty, true))
        .collect::<Vec<_>>()
        .join(",");
    Symbol {
        fqn: Arc::from(format!(
            "{}.{}({})",
            class_fqn, function.name, fqn_parameters
        )),
        simple_name: Arc::from(function.name.as_str()),
        range: TextRange::new(0, 0),
        kind: if is_package {
            SymbolKind::FUNCTION
        } else {
            SymbolKind::METHOD
        },
        modifiers: method
            .map(method_modifiers)
            .unwrap_or_else(|| fallback_kotlin_modifiers(is_package)),
        container: Some(Arc::from(class_fqn)),
        return_type: Some(Arc::from(kotlin_type_display(&function.return_type, true))),
        parameters,
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: kotlin_type_parameters(&function.type_parameters),
        is_deprecated: false,
        receiver_type: function
            .extension_receiver_type
            .as_ref()
            .map(|ty| Arc::from(kotlin_type_display(ty, false))),
        throws: None,
        kotlin: Some(Box::new(KotlinSymbolMetadata {
            return_type_nullable: function.return_type.nullable,
            receiver_type_nullable: function
                .extension_receiver_type
                .as_ref()
                .is_some_and(|ty| ty.nullable),
            parameters: function
                .parameters
                .iter()
                .map(kotlin_parameter_metadata)
                .collect(),
            is_suspend: function.is_suspend,
            is_operator: function.is_operator,
            is_inline: function.is_inline,
            ..KotlinSymbolMetadata::default()
        })),
    }
}

fn convert_kotlin_constructor(
    constructor: &KotlinConstructorInfo,
    method: Option<&MethodSymbol>,
    class_fqn: &str,
) -> Symbol {
    let simple_class_name = extract_simple_name(class_fqn);
    let fqn_parameters = constructor
        .parameters
        .iter()
        .map(|parameter| kotlin_type_display(&parameter.ty, true))
        .collect::<Vec<_>>()
        .join(",");
    Symbol {
        fqn: Arc::from(format!(
            "{}.{}({})",
            class_fqn, simple_class_name, fqn_parameters
        )),
        simple_name: Arc::from(simple_class_name),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CONSTRUCTOR,
        modifiers: method
            .map(method_modifiers)
            .unwrap_or_else(|| fallback_kotlin_modifiers(false)),
        container: Some(Arc::from(class_fqn)),
        return_type: Some(Arc::from(class_fqn)),
        parameters: constructor
            .parameters
            .iter()
            .map(kotlin_parameter_display)
            .map(Arc::from)
            .collect(),
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None,
        throws: None,
        kotlin: Some(Box::new(KotlinSymbolMetadata {
            parameters: constructor
                .parameters
                .iter()
                .map(kotlin_parameter_metadata)
                .collect(),
            ..KotlinSymbolMetadata::default()
        })),
    }
}

fn convert_kotlin_property(
    property: &KotlinPropertyInfo,
    method: Option<&MethodSymbol>,
    field: Option<&FieldSymbol>,
    class_fqn: &str,
    is_package: bool,
) -> Symbol {
    let modifiers = method
        .map(method_modifiers)
        .or_else(|| field.map(field_modifiers))
        .unwrap_or_else(|| fallback_kotlin_modifiers(is_package));
    Symbol {
        fqn: Arc::from(format!("{}.{}", class_fqn, property.name)),
        simple_name: Arc::from(property.name.as_str()),
        range: TextRange::new(0, 0),
        kind: SymbolKind::PROPERTY,
        modifiers,
        container: Some(Arc::from(class_fqn)),
        return_type: Some(Arc::from(kotlin_type_display(&property.ty, true))),
        parameters: Vec::new(),
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: property
            .extension_receiver_type
            .as_ref()
            .map(|ty| Arc::from(kotlin_type_display(ty, false))),
        throws: None,
        kotlin: Some(Box::new(KotlinSymbolMetadata {
            return_type_nullable: property.ty.nullable,
            receiver_type_nullable: property
                .extension_receiver_type
                .as_ref()
                .is_some_and(|ty| ty.nullable),
            property: Some(KotlinPropertyMetadata {
                is_val: property.is_val,
                has_getter: property.has_getter,
                has_setter: property.has_setter,
            }),
            ..KotlinSymbolMetadata::default()
        })),
    }
}

fn method_modifiers(method: &MethodSymbol) -> Modifiers {
    Modifiers {
        is_public: method.is_public,
        is_private: method.is_private,
        is_protected: method.is_protected,
        is_static: method.is_static,
        is_final: method.is_final,
        is_abstract: method.is_abstract,
        ..Modifiers::default()
    }
}

fn field_modifiers(field: &FieldSymbol) -> Modifiers {
    Modifiers {
        is_public: field.is_public,
        is_private: field.is_private,
        is_protected: field.is_protected,
        is_static: field.is_static,
        is_final: field.is_final,
        ..Modifiers::default()
    }
}

fn fallback_kotlin_modifiers(is_package: bool) -> Modifiers {
    Modifiers {
        is_public: true,
        is_static: is_package,
        ..Modifiers::default()
    }
}

fn kotlin_parameter_display(parameter: &KotlinParameterInfo) -> String {
    format!(
        "{} {}",
        kotlin_type_display(&parameter.ty, true),
        parameter.name
    )
}

fn kotlin_parameter_metadata(parameter: &KotlinParameterInfo) -> KotlinParameterMetadata {
    KotlinParameterMetadata {
        nullable: parameter.ty.nullable,
        has_default: parameter.has_default,
    }
}

/// The typealiases a file facade declares that another module can name.
///
/// They belong to the file's Kotlin package: `pn` when `@JvmPackageName` moved
/// the facade, otherwise the facade's own package. Private and internal aliases
/// are skipped, and so is an alias that expands to a type parameter, which has
/// no class to resolve to. The alias's type parameters and the expansion's
/// type arguments are kept; see [`kotlin_type_alias_arguments`].
fn kotlin_package_type_aliases(
    facade_fqn: &str,
    info: &KotlinClassInfo,
) -> Vec<crate::KotlinTypeAlias> {
    let package = info
        .metadata
        .package_name
        .as_deref()
        .filter(|package| !package.is_empty())
        .map(|package| package.replace('/', "."))
        .or_else(|| {
            facade_fqn
                .rsplit_once('.')
                .map(|(package, _)| package.to_owned())
        });
    info.type_aliases
        .iter()
        .filter(|alias| alias.is_public)
        .filter_map(|alias| {
            let KotlinClassifier::Class(expanded_class) = &alias.expanded_type.classifier else {
                return None;
            };
            // An argument that names a parameter the alias does not declare
            // cannot be bound at a use, so neither the parameters nor the
            // arguments are recorded.
            let (type_parameters, expanded_type_arguments) =
                kotlin_type_alias_arguments(&alias.type_parameters, &alias.expanded_type)
                    .map(|arguments| {
                        let parameters = alias
                            .type_parameters
                            .iter()
                            .map(|parameter| parameter.name.clone())
                            .collect();
                        (parameters, arguments)
                    })
                    .unwrap_or_default();
            Some(crate::KotlinTypeAlias {
                fqn: package.as_deref().map_or_else(
                    || alias.name.clone(),
                    |package| format!("{package}.{}", alias.name),
                ),
                expanded_class: expanded_class.clone(),
                type_parameters,
                expanded_type_arguments,
            })
        })
        .collect()
}

/// The type arguments a typealias's expansion gives its class, as qualified
/// Kotlin spellings: `["kotlin.String"]` for `java.util.ArrayList<kotlin.String>`.
///
/// A type parameter of the alias is spelled by its name, which a use binds
/// (`["E"]` for `typealias ArrayList<E> = java.util.ArrayList<E>`). `None`
/// when an argument names a type parameter that is not among `parameters`,
/// or a classifier the metadata does not identify.
fn kotlin_type_alias_arguments(
    parameters: &[KotlinTypeParameterInfo],
    expanded: &KotlinTypeInfo,
) -> Option<Vec<String>> {
    fn spelling(
        argument: &KotlinTypeArgument,
        parameters: &[KotlinTypeParameterInfo],
    ) -> Option<String> {
        let Some(ty) = &argument.ty else {
            return Some("*".to_owned());
        };
        let mut rendered = match argument.variance {
            KotlinVariance::Invariant => String::new(),
            KotlinVariance::Out => "out ".to_owned(),
            KotlinVariance::In => "in ".to_owned(),
        };
        let name = match &ty.classifier {
            KotlinClassifier::Class(name) | KotlinClassifier::TypeAlias(name) => name.as_str(),
            KotlinClassifier::TypeParameter(id) => parameters
                .iter()
                .find(|parameter| parameter.id == *id)
                .map(|parameter| parameter.name.as_str())?,
            KotlinClassifier::TypeParameterName(name) => parameters
                .iter()
                .find(|parameter| parameter.name == *name)
                .map(|parameter| parameter.name.as_str())?,
            KotlinClassifier::Unknown => return None,
        };
        rendered.push_str(name);
        if !ty.arguments.is_empty() {
            let arguments = ty
                .arguments
                .iter()
                .map(|argument| spelling(argument, parameters))
                .collect::<Option<Vec<_>>>()?;
            rendered.push('<');
            rendered.push_str(&arguments.join(", "));
            rendered.push('>');
        }
        if ty.nullable {
            rendered.push('?');
        }
        Some(rendered)
    }
    expanded
        .arguments
        .iter()
        .map(|argument| spelling(argument, parameters))
        .collect()
}

fn kotlin_type_parameters(parameters: &[KotlinTypeParameterInfo]) -> Vec<TypeParameterInfo> {
    parameters
        .iter()
        .enumerate()
        .map(|(position, parameter)| {
            TypeParameterInfo::with_bounds(
                parameter.name.clone(),
                position,
                parameter
                    .upper_bounds
                    .iter()
                    .map(|bound| Arc::from(kotlin_type_display(bound, true))),
            )
        })
        .collect()
}

fn kotlin_type_display(ty: &KotlinTypeInfo, include_nullability: bool) -> String {
    let mut display = match &ty.classifier {
        KotlinClassifier::Class(name) | KotlinClassifier::TypeAlias(name) => name.clone(),
        KotlinClassifier::TypeParameter(id) => format!("T{id}"),
        KotlinClassifier::TypeParameterName(name) => name.clone(),
        KotlinClassifier::Unknown => "kotlin.Any".to_string(),
    };
    if !ty.arguments.is_empty() {
        let arguments = ty
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .ty
                    .as_ref()
                    .map(|ty| kotlin_type_display(ty, true))
                    .unwrap_or_else(|| "*".to_string())
            })
            .collect::<Vec<_>>()
            .join(", ");
        display.push('<');
        display.push_str(&arguments);
        display.push('>');
    }
    if include_nullability && ty.nullable {
        display.push('?');
    }
    display
}

fn find_function_method(
    class: &ClassSymbols,
    info: &KotlinClassInfo,
    function: &KotlinFunctionInfo,
) -> Option<usize> {
    let signature_name = function
        .jvm_signature
        .as_ref()
        .map_or(function.name.as_str(), |signature| signature.name.as_str());
    let synthesized = synthesized_function_descriptor(function, &info.type_parameters);
    find_method(
        class,
        signature_name,
        function.jvm_signature.as_ref(),
        synthesized.as_deref(),
        function.parameters.len()
            + usize::from(function.extension_receiver_type.is_some())
            + usize::from(function.is_suspend),
    )
}

fn find_constructor_method(
    class: &ClassSymbols,
    info: &KotlinClassInfo,
    constructor: &KotlinConstructorInfo,
) -> Option<usize> {
    let synthesized = synthesized_constructor_descriptor(constructor, &info.type_parameters);
    find_method(
        class,
        "<init>",
        constructor.jvm_signature.as_ref(),
        synthesized.as_deref(),
        constructor.parameters.len(),
    )
}

fn find_method(
    class: &ClassSymbols,
    fallback_name: &str,
    explicit: Option<&KotlinJvmMethodSignature>,
    synthesized_descriptor: Option<&str>,
    expected_parameter_count: usize,
) -> Option<usize> {
    let name = explicit.map_or(fallback_name, |signature| signature.name.as_str());
    let candidates = class
        .methods
        .iter()
        .enumerate()
        .filter(|(_, method)| method.name == name)
        .collect::<Vec<_>>();
    if let Some(descriptor) = explicit.and_then(|signature| signature.descriptor.as_deref()) {
        if let Some((index, _)) = candidates
            .iter()
            .find(|(_, method)| method_descriptor(method) == descriptor)
        {
            return Some(*index);
        }
    }
    if let Some(descriptor) = synthesized_descriptor {
        if let Some((index, _)) = candidates
            .iter()
            .find(|(_, method)| method_descriptor(method) == descriptor)
        {
            return Some(*index);
        }
    }
    let mut by_arity = candidates
        .into_iter()
        .filter(|(_, method)| method.parameters.len() == expected_parameter_count);
    let first = by_arity.next()?;
    by_arity.next().is_none().then_some(first.0)
}

#[derive(Default)]
struct PropertyMembers {
    methods: Vec<usize>,
    field: Option<usize>,
}

fn find_property_members(
    class: &ClassSymbols,
    info: &KotlinClassInfo,
    property: &KotlinPropertyInfo,
) -> PropertyMembers {
    let mut members = PropertyMembers::default();
    if let Some(signatures) = &property.jvm_signature {
        let receiver_count = usize::from(property.extension_receiver_type.is_some());
        for (signature, parameter_count) in [
            (signatures.getter.as_ref(), receiver_count),
            (signatures.setter.as_ref(), receiver_count + 1),
            (signatures.synthetic_method.as_ref(), 0),
            (signatures.delegate_method.as_ref(), 0),
        ] {
            let Some(signature) = signature else {
                continue;
            };
            if let Some(index) = find_method(
                class,
                &signature.name,
                Some(signature),
                None,
                parameter_count,
            ) {
                if !members.methods.contains(&index) {
                    members.methods.push(index);
                }
            }
        }
        if let Some(signature) = &signatures.field {
            members.field = class.fields.iter().enumerate().find_map(|(index, field)| {
                (field.name == signature.name
                    && signature.descriptor.as_deref().is_none_or(|descriptor| {
                        descriptor_type_to_jvm(&field.field_type) == descriptor
                    }))
                .then_some(index)
            });
        }
    }

    if members.field.is_none() {
        members.field = class
            .fields
            .iter()
            .position(|field| field.name == property.name);
    }

    let capitalized = capitalize_identifier(&property.name);
    let getter_name = if property.name.starts_with("is")
        && property.name.chars().nth(2).is_some_and(char::is_uppercase)
    {
        property.name.clone()
    } else {
        format!("get{capitalized}")
    };
    let setter_name = format!("set{capitalized}");
    for (index, method) in class.methods.iter().enumerate() {
        if (method.name == getter_name || (!property.is_val && method.name == setter_name))
            && !members.methods.contains(&index)
        {
            members.methods.push(index);
        }
    }

    // Force descriptor synthesis for derivable accessor signatures too. It
    // narrows overloads even when the JVM extension omitted the descriptor.
    let receiver_count = usize::from(property.extension_receiver_type.is_some());
    let getter_descriptor = synthesized_property_getter_descriptor(property, &info.type_parameters);
    let setter_descriptor = synthesized_property_setter_descriptor(property, &info.type_parameters);
    members.methods.retain(|index| {
        let method = &class.methods[*index];
        if method.name == getter_name {
            getter_descriptor
                .as_deref()
                .is_none_or(|descriptor| method_descriptor(method) == descriptor)
                || method.parameters.len() == receiver_count
        } else if method.name == setter_name {
            setter_descriptor
                .as_deref()
                .is_none_or(|descriptor| method_descriptor(method) == descriptor)
                || method.parameters.len() == receiver_count + 1
        } else {
            true
        }
    });
    members
}

fn capitalize_identifier(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

fn synthesized_function_descriptor(
    function: &KotlinFunctionInfo,
    class_type_parameters: &[KotlinTypeParameterInfo],
) -> Option<String> {
    let mut descriptor = String::from("(");
    if let Some(receiver) = &function.extension_receiver_type {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            receiver,
            &function.type_parameters,
            class_type_parameters,
            false,
        )?);
    }
    for parameter in &function.parameters {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            &parameter.ty,
            &function.type_parameters,
            class_type_parameters,
            false,
        )?);
    }
    if function.is_suspend {
        descriptor.push_str("Lkotlin/coroutines/Continuation;");
    }
    descriptor.push(')');
    if function.is_suspend {
        descriptor.push_str("Ljava/lang/Object;");
    } else {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            &function.return_type,
            &function.type_parameters,
            class_type_parameters,
            true,
        )?);
    }
    Some(descriptor)
}

fn synthesized_constructor_descriptor(
    constructor: &KotlinConstructorInfo,
    class_type_parameters: &[KotlinTypeParameterInfo],
) -> Option<String> {
    let mut descriptor = String::from("(");
    for parameter in &constructor.parameters {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            &parameter.ty,
            &[],
            class_type_parameters,
            false,
        )?);
    }
    descriptor.push_str(")V");
    Some(descriptor)
}

fn synthesized_property_getter_descriptor(
    property: &KotlinPropertyInfo,
    class_type_parameters: &[KotlinTypeParameterInfo],
) -> Option<String> {
    let mut descriptor = String::from("(");
    if let Some(receiver) = &property.extension_receiver_type {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            receiver,
            &[],
            class_type_parameters,
            false,
        )?);
    }
    descriptor.push(')');
    descriptor.push_str(&kotlin_type_to_jvm_descriptor(
        &property.ty,
        &[],
        class_type_parameters,
        true,
    )?);
    Some(descriptor)
}

fn synthesized_property_setter_descriptor(
    property: &KotlinPropertyInfo,
    class_type_parameters: &[KotlinTypeParameterInfo],
) -> Option<String> {
    let mut descriptor = String::from("(");
    if let Some(receiver) = &property.extension_receiver_type {
        descriptor.push_str(&kotlin_type_to_jvm_descriptor(
            receiver,
            &[],
            class_type_parameters,
            false,
        )?);
    }
    descriptor.push_str(&kotlin_type_to_jvm_descriptor(
        &property.ty,
        &[],
        class_type_parameters,
        false,
    )?);
    descriptor.push_str(")V");
    Some(descriptor)
}

fn kotlin_type_to_jvm_descriptor(
    ty: &KotlinTypeInfo,
    function_type_parameters: &[KotlinTypeParameterInfo],
    class_type_parameters: &[KotlinTypeParameterInfo],
    allow_void: bool,
) -> Option<String> {
    match &ty.classifier {
        KotlinClassifier::TypeParameter(id) => {
            let parameter = function_type_parameters
                .iter()
                .chain(class_type_parameters)
                .find(|parameter| parameter.id == *id);
            if let Some(bound) = parameter.and_then(|parameter| parameter.upper_bounds.first()) {
                kotlin_type_to_jvm_descriptor(
                    bound,
                    function_type_parameters,
                    class_type_parameters,
                    false,
                )
            } else {
                Some("Ljava/lang/Object;".to_string())
            }
        }
        KotlinClassifier::TypeParameterName(_) | KotlinClassifier::Unknown => {
            Some("Ljava/lang/Object;".to_string())
        }
        KotlinClassifier::Class(name) | KotlinClassifier::TypeAlias(name) => {
            kotlin_class_to_jvm_descriptor(name, ty, allow_void)
        }
    }
}

fn kotlin_class_to_jvm_descriptor(
    name: &str,
    ty: &KotlinTypeInfo,
    allow_void: bool,
) -> Option<String> {
    let primitive = match name {
        "kotlin.Boolean" => Some(("Z", "java/lang/Boolean")),
        "kotlin.Byte" => Some(("B", "java/lang/Byte")),
        "kotlin.Char" => Some(("C", "java/lang/Character")),
        "kotlin.Short" => Some(("S", "java/lang/Short")),
        "kotlin.Int" => Some(("I", "java/lang/Integer")),
        "kotlin.Float" => Some(("F", "java/lang/Float")),
        "kotlin.Long" => Some(("J", "java/lang/Long")),
        "kotlin.Double" => Some(("D", "java/lang/Double")),
        _ => None,
    };
    if let Some((primitive_descriptor, boxed_name)) = primitive {
        return Some(if ty.nullable {
            format!("L{boxed_name};")
        } else {
            primitive_descriptor.to_string()
        });
    }
    if name == "kotlin.Unit" {
        return Some(if allow_void && !ty.nullable {
            "V".to_string()
        } else {
            "Lkotlin/Unit;".to_string()
        });
    }
    if let Some(element) = name.strip_prefix("kotlin.").and_then(|name| match name {
        "BooleanArray" => Some("Z"),
        "ByteArray" => Some("B"),
        "CharArray" => Some("C"),
        "ShortArray" => Some("S"),
        "IntArray" => Some("I"),
        "FloatArray" => Some("F"),
        "LongArray" => Some("J"),
        "DoubleArray" => Some("D"),
        _ => None,
    }) {
        return Some(format!("[{element}"));
    }
    if name == "kotlin.Array" {
        let element = ty
            .arguments
            .first()
            .and_then(|argument| argument.ty.as_ref())
            .and_then(boxed_array_element_descriptor)
            .unwrap_or_else(|| "Ljava/lang/Object;".to_string());
        return Some(format!("[{element}"));
    }

    let internal_name = match crate::kotlin_builtin_jvm_fqn(name) {
        Some("java.util.Map.Entry") => "java/util/Map$Entry".to_owned(),
        Some(mapped) => mapped.replace('.', "/"),
        _ if name.starts_with("kotlin.Function") => {
            name.replacen("kotlin.Function", "kotlin/jvm/functions/Function", 1)
        }
        _ => name.replace('.', "/"),
    };
    Some(format!("L{internal_name};"))
}

fn boxed_array_element_descriptor(ty: &KotlinTypeInfo) -> Option<String> {
    let mut boxed = ty.clone();
    boxed.nullable = true;
    kotlin_type_to_jvm_descriptor(&boxed, &[], &[], false)
}

fn method_descriptor(method: &MethodSymbol) -> String {
    let mut descriptor = String::from("(");
    for parameter in &method.parameters {
        descriptor.push_str(&descriptor_type_to_jvm(parameter));
    }
    descriptor.push(')');
    descriptor.push_str(&descriptor_type_to_jvm(&method.return_type));
    descriptor
}

fn descriptor_type_to_jvm(ty: &DescriptorType) -> String {
    match ty {
        DescriptorType::Byte => "B".to_string(),
        DescriptorType::Char => "C".to_string(),
        DescriptorType::Double => "D".to_string(),
        DescriptorType::Float => "F".to_string(),
        DescriptorType::Int => "I".to_string(),
        DescriptorType::Long => "J".to_string(),
        DescriptorType::Short => "S".to_string(),
        DescriptorType::Boolean => "Z".to_string(),
        DescriptorType::Void => "V".to_string(),
        DescriptorType::Class { name } => format!("L{};", name.replace('.', "/")),
        DescriptorType::Array {
            element_type,
            dimensions,
        } => format!(
            "{}{}",
            "[".repeat(*dimensions),
            descriptor_type_to_jvm(element_type)
        ),
    }
}

fn is_kotlin_lowering_method(method: &MethodSymbol, info: &KotlinClassInfo) -> bool {
    method.name.ends_with("$default")
        || method.name.starts_with("access$")
        || info.functions.iter().any(|function| {
            function.name == method.name
                || function
                    .jvm_signature
                    .as_ref()
                    .is_some_and(|signature| signature.name == method.name)
        })
        || info.properties.iter().any(|property| {
            let capitalized = capitalize_identifier(&property.name);
            method.name == format!("get{capitalized}")
                || method.name == format!("set{capitalized}")
                || method.name == property.name
        })
}

/// Convert bytecode class symbols to workspace symbols
///
/// Each class produces multiple symbols:
/// - One class symbol
/// - One symbol per method (with parameter names when available)
/// - One symbol per field
pub(crate) fn convert_java_class_symbols(class: &ClassSymbols) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    // Add the class symbol
    symbols.push(convert_class_symbol(class));

    // Add method symbols
    for method in &class.methods {
        if let Some(symbol) = convert_method_symbol(method, &class.fqn) {
            symbols.push(symbol);
        }
    }

    // Add field symbols
    for field in &class.fields {
        if let Some(symbol) = convert_field_symbol(field, &class.fqn) {
            symbols.push(symbol);
        }
    }

    symbols
}

/// Convert a bytecode method symbol to a workspace symbol
///
/// CRITICAL: Extracts parameter names from LocalVariableTable attribute when available.
/// This is what enables real parameter names in inlay hints instead of arg0/arg1.
pub(crate) fn convert_method_symbol(method: &MethodSymbol, class_fqn: &str) -> Option<Symbol> {
    // Bridges and other compiler artifacts are part of JVM linkage, not the
    // source-language member set. In particular, a covariant/generic bridge
    // has the same Java signature as its implementation once return types are
    // excluded from the key, so publishing both makes the elected return type
    // depend on class-file order.
    if method.is_bridge || method.is_synthetic {
        return None;
    }

    // Preserve parameter type identity and combine it with parameter names
    // Format: "Type name" when parameter name is available, "Type" otherwise
    let parameters: Vec<String> = method
        .parameters
        .iter()
        .enumerate()
        .map(|(i, param_type)| {
            let type_name = method
                .generic_parameter_types
                .get(i)
                .cloned()
                .unwrap_or_else(|| descriptor_to_source_type(param_type));
            let type_name = source_parameter_type(method, i, type_name);

            // If we have a parameter name from LocalVariableTable, use "Type name" format
            // This is what makes inlay hints show real names!
            if let Some(Some(param_name)) = method.parameter_names.get(i) {
                format!("{} {}", type_name, param_name)
            } else {
                // Fallback to arg0, arg1, etc. for methods without debug info
                format!("{} arg{}", type_name, i)
            }
        })
        .collect();

    // For method FQN signature, use only types (without names)
    let param_types: Vec<String> = method
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            source_parameter_type(method, index, descriptor_to_source_type(parameter))
        })
        .collect();
    let param_descriptor = param_types.join(",");

    let is_constructor = method.is_constructor();
    let simple_name = if is_constructor {
        extract_simple_name(class_fqn)
    } else {
        method.name.clone()
    };
    let method_fqn = format!("{}.{}({})", class_fqn, simple_name, param_descriptor);

    // Return types participate in subtype and override checks, so preserve
    // canonical identity just as method FQN parameter descriptors do. Generic
    // signatures are canonicalized by the bytecode extractor.
    let return_type = if is_constructor {
        Some(class_fqn.to_string())
    } else {
        method
            .generic_return_type
            .clone()
            .or_else(|| Some(descriptor_to_source_type(&method.return_type)))
    };

    Some(Symbol {
        fqn: Arc::from(method_fqn),
        simple_name: Arc::from(simple_name),
        range: TextRange::new(0, 0),
        kind: if is_constructor {
            SymbolKind::CONSTRUCTOR
        } else {
            SymbolKind::METHOD
        },
        modifiers: Modifiers {
            is_public: method.is_public,
            is_private: method.is_private,
            is_protected: method.is_protected,
            is_static: method.is_static,
            is_final: method.is_final,
            is_abstract: method.is_abstract,
            is_sealed: false,
            is_data: false,
            ..Modifiers::default()
        },
        container: Some(Arc::from(class_fqn)),
        return_type: return_type.map(Into::into),
        parameters: parameters.into_iter().map(Into::into).collect(), // Now includes "Type name" format when available!
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: method
            .type_parameters
            .iter()
            .enumerate()
            .map(|(position, name)| crate::TypeParameterInfo::new(name.clone(), position))
            .collect(),
        is_deprecated: method.is_deprecated,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        // The bytecode `Exceptions` attribute, with nested classes in source
        // form (`java.util.concurrent.ExecutionException`, `Outer.Inner`).
        throws: (!method.exceptions.is_empty()).then(|| {
            Arc::new(
                method
                    .exceptions
                    .iter()
                    .map(|name| Arc::<str>::from(name.replace('$', ".")))
                    .collect(),
            )
        }),
    })
}

fn source_parameter_type(method: &MethodSymbol, index: usize, mut type_name: String) -> String {
    if method.is_varargs && index + 1 == method.parameters.len() {
        if let Some(element_type) = type_name.strip_suffix("[]") {
            type_name = format!("{element_type}...");
        }
    }
    type_name
}

/// Convert a bytecode field symbol to a workspace symbol
fn convert_field_symbol(field: &FieldSymbol, class_fqn: &str) -> Option<Symbol> {
    // Build field FQN: "com.example.Class.fieldName"
    let field_fqn = format!("{}.{}", class_fqn, field.name);

    // Field type
    let field_type = descriptor_to_source_type(&field.field_type);

    Some(Symbol {
        fqn: Arc::from(field_fqn),
        simple_name: Arc::from(field.name.as_str()),
        range: TextRange::new(0, 0),
        kind: SymbolKind::FIELD,
        modifiers: Modifiers {
            is_public: field.is_public,
            is_private: field.is_private,
            is_protected: field.is_protected,
            is_static: field.is_static,
            is_final: field.is_final,
            is_abstract: false,
            is_sealed: false,
            is_data: false,
            ..Modifiers::default()
        },
        container: Some(Arc::from(class_fqn)),
        return_type: Some(field_type.into()),
        parameters: Vec::new(),
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: Vec::new(),
        is_deprecated: field.is_deprecated,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_bytecode::descriptor::DescriptorType;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[test]
    fn cumulative_archive_budget_rejects_each_dimension() {
        let mut classes = ArchiveWorkBudget::with_limits("test", 1, 100, 100);
        classes.record_class(1).unwrap();
        assert!(classes
            .record_class(1)
            .unwrap_err()
            .to_string()
            .contains("class entries"));

        let mut bytes = ArchiveWorkBudget::with_limits("test", 10, 5, 100);
        bytes.record_class(3).unwrap();
        assert!(bytes
            .record_class(3)
            .unwrap_err()
            .to_string()
            .contains("total bytes"));

        let mut symbols = ArchiveWorkBudget::with_limits("test", 10, 100, 2);
        symbols.record_symbols(2).unwrap();
        assert!(symbols
            .record_symbols(1)
            .unwrap_err()
            .to_string()
            .contains("symbols"));
    }

    fn compile_test_jar(root: &Path, class_name: &str) -> PathBuf {
        let fixture_root = root.join(class_name);
        let source_dir = fixture_root.join("src/snapshot");
        let classes_dir = fixture_root.join("classes");
        std::fs::create_dir_all(&source_dir).expect("create Java source directory");
        std::fs::create_dir_all(&classes_dir).expect("create Java classes directory");
        let source_path = source_dir.join(format!("{class_name}.java"));
        std::fs::write(
            &source_path,
            format!("package snapshot; public class {class_name} {{}}"),
        )
        .expect("write Java source fixture");

        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&source_path)
            .output()
            .expect("run javac for archive fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let jar_path = fixture_root.join(format!("{class_name}.jar"));
        let jar_file = std::fs::File::create(&jar_path).expect("create fixture JAR");
        let mut jar = zip::ZipWriter::new(jar_file);
        jar.start_file(
            format!("snapshot/{class_name}.class"),
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start class entry");
        jar.write_all(
            &std::fs::read(classes_dir.join(format!("snapshot/{class_name}.class")))
                .expect("read compiled class"),
        )
        .expect("write class entry");
        jar.finish().expect("finish fixture JAR");
        jar_path
    }

    fn kotlin_class_type(name: &str) -> KotlinTypeInfo {
        KotlinTypeInfo {
            classifier: KotlinClassifier::Class(name.to_string()),
            arguments: Vec::new(),
            nullable: false,
            is_suspend: false,
        }
    }

    /// A library alias keeps its expansion's type arguments as
    /// qualified Kotlin spellings, and a generic alias spells each of its own
    /// type parameters by name; a parameter it does not declare gives none.
    #[test]
    fn library_typealias_arguments_name_the_alias_type_parameters() {
        let argument = |variance, ty| KotlinTypeArgument { variance, ty };
        let generic = |name: &str, arguments| KotlinTypeInfo {
            arguments,
            ..kotlin_class_type(name)
        };
        let nullable_string = KotlinTypeInfo {
            nullable: true,
            ..kotlin_class_type("kotlin.String")
        };
        let expanded = generic(
            "java.util.HashMap",
            vec![
                argument(KotlinVariance::Invariant, Some(nullable_string)),
                argument(
                    KotlinVariance::Invariant,
                    Some(generic(
                        "kotlin.collections.List",
                        vec![argument(
                            KotlinVariance::Out,
                            Some(kotlin_class_type("kotlin.Number")),
                        )],
                    )),
                ),
                argument(KotlinVariance::Invariant, None),
            ],
        );
        assert_eq!(
            kotlin_type_alias_arguments(&[], &expanded),
            Some(vec![
                "kotlin.String?".to_owned(),
                "kotlin.collections.List<out kotlin.Number>".to_owned(),
                "*".to_owned()
            ])
        );
        assert_eq!(
            kotlin_type_alias_arguments(
                &[],
                &generic(
                    "java.util.ArrayList",
                    vec![argument(
                        KotlinVariance::Out,
                        Some(kotlin_class_type("kotlin.Number"))
                    )],
                )
            ),
            Some(vec!["out kotlin.Number".to_owned()])
        );

        // `typealias Inverse<K, V> = java.util.HashMap<V, kotlin.collections.List<K>?>`.
        let parameter = |id: u32, name: &str| KotlinTypeParameterInfo {
            id,
            name: name.to_owned(),
            variance: KotlinVariance::Invariant,
            is_reified: false,
            upper_bounds: Vec::new(),
        };
        let reference = |id| KotlinTypeInfo {
            classifier: KotlinClassifier::TypeParameter(id),
            ..kotlin_class_type("")
        };
        let inverse = generic(
            "java.util.HashMap",
            vec![
                argument(KotlinVariance::Invariant, Some(reference(1))),
                argument(
                    KotlinVariance::Invariant,
                    Some(KotlinTypeInfo {
                        nullable: true,
                        ..generic(
                            "kotlin.collections.List",
                            vec![argument(KotlinVariance::Invariant, Some(reference(0)))],
                        )
                    }),
                ),
            ],
        );
        let parameters = [parameter(0, "K"), parameter(1, "V")];
        assert_eq!(
            kotlin_type_alias_arguments(&parameters, &inverse),
            Some(vec![
                "V".to_owned(),
                "kotlin.collections.List<K>?".to_owned()
            ])
        );
        assert_eq!(
            kotlin_type_alias_arguments(&parameters[..1], &inverse),
            None,
            "V is not a parameter of this alias"
        );
        assert_eq!(
            kotlin_type_alias_arguments(&[], &kotlin_class_type("java.lang.StringBuilder")),
            Some(Vec::new())
        );
    }

    fn synthetic_bytecode_method(name: &str, parameters: Vec<DescriptorType>) -> MethodSymbol {
        MethodSymbol {
            name: name.to_string(),
            parameter_names: vec![None; parameters.len()],
            parameters,
            return_type: DescriptorType::Void,
            generic_return_type: None,
            generic_parameter_types: Vec::new(),
            type_parameters: Vec::new(),
            exceptions: Vec::new(),
            is_public: true,
            is_private: false,
            is_protected: false,
            is_static: false,
            is_final: false,
            is_abstract: false,
            is_native: false,
            is_bridge: false,
            is_synthetic: false,
            is_varargs: false,
            is_deprecated: false,
        }
    }

    #[test]
    fn test_extract_simple_name() {
        assert_eq!(extract_simple_name("java.lang.String"), "String");
        assert_eq!(extract_simple_name("com.example.User"), "User");
        assert_eq!(extract_simple_name("User"), "User");
        assert_eq!(extract_simple_name(""), "");
    }

    #[test]
    fn archive_entry_filter_skips_metadata_and_multi_release_overlays() {
        assert!(is_indexable_class_entry("com/example/User.class"));
        assert!(!is_indexable_class_entry("module-info.class"));
        assert!(!is_indexable_class_entry("com/example/package-info.class"));
        assert!(!is_indexable_class_entry(
            "META-INF/versions/17/com/example/User.class"
        ));
        assert!(!is_indexable_class_entry("META-INF/MANIFEST.MF"));
    }

    #[test]
    fn test_descriptor_to_source_type() {
        assert_eq!(descriptor_to_source_type(&DescriptorType::Int), "int");
        assert_eq!(
            descriptor_to_source_type(&DescriptorType::Boolean),
            "boolean"
        );
        assert_eq!(
            descriptor_to_source_type(&DescriptorType::Class {
                name: "java.lang.String".into()
            }),
            "java.lang.String"
        );
        assert_eq!(
            descriptor_to_source_type(&DescriptorType::Array {
                element_type: Box::new(DescriptorType::Int),
                dimensions: 2
            }),
            "int[][]"
        );
    }

    #[test]
    fn java_method_keys_retain_qualified_parameter_identity() {
        let util_date = synthetic_bytecode_method(
            "accept",
            vec![DescriptorType::Class {
                name: "java.util.Date".to_string(),
            }],
        );
        let sql_date = synthetic_bytecode_method(
            "accept",
            vec![DescriptorType::Class {
                name: "java.sql.Date".to_string(),
            }],
        );

        let util_symbol = convert_method_symbol(&util_date, "sample.Service").unwrap();
        let sql_symbol = convert_method_symbol(&sql_date, "sample.Service").unwrap();

        assert_eq!(
            util_symbol.fqn.as_ref(),
            "sample.Service.accept(java.util.Date)"
        );
        assert_eq!(
            sql_symbol.fqn.as_ref(),
            "sample.Service.accept(java.sql.Date)"
        );
        assert_ne!(util_symbol.fqn, sql_symbol.fqn);
        assert_eq!(util_symbol.parameters[0].as_ref(), "java.util.Date arg0");
    }

    #[test]
    fn java_bytecode_constructors_use_source_identity_and_kind() {
        let constructor = synthetic_bytecode_method(
            "<init>",
            vec![DescriptorType::Class {
                name: "java.lang.String".to_string(),
            }],
        );

        let symbol = convert_method_symbol(&constructor, "sample.Service").unwrap();

        assert_eq!(symbol.kind, SymbolKind::CONSTRUCTOR);
        assert_eq!(symbol.simple_name.as_ref(), "Service");
        assert_eq!(
            symbol.fqn.as_ref(),
            "sample.Service.Service(java.lang.String)"
        );
        assert_eq!(symbol.return_type.as_deref(), Some("sample.Service"));
    }

    #[test]
    fn java_bytecode_varargs_recover_source_syntax_without_reclassifying_arrays() {
        let array = DescriptorType::Array {
            element_type: Box::new(DescriptorType::Class {
                name: "java.lang.String".to_string(),
            }),
            dimensions: 1,
        };
        let ordinary_array = synthetic_bytecode_method("accept", vec![array.clone()]);
        let mut varargs = synthetic_bytecode_method("accept", vec![array]);
        varargs.is_varargs = true;

        let array_symbol = convert_method_symbol(&ordinary_array, "sample.Service").unwrap();
        let varargs_symbol = convert_method_symbol(&varargs, "sample.Service").unwrap();

        assert_eq!(
            array_symbol.fqn.as_ref(),
            "sample.Service.accept(java.lang.String[])"
        );
        assert_eq!(
            array_symbol.parameters[0].as_ref(),
            "java.lang.String[] arg0"
        );
        assert_eq!(
            varargs_symbol.fqn.as_ref(),
            "sample.Service.accept(java.lang.String...)"
        );
        assert_eq!(
            varargs_symbol.parameters[0].as_ref(),
            "java.lang.String... arg0"
        );
    }

    #[test]
    fn java_bytecode_compiler_artifacts_are_not_source_members() {
        let mut bridge = synthetic_bytecode_method("value", Vec::new());
        bridge.is_bridge = true;
        let mut synthetic = synthetic_bytecode_method("access$000", Vec::new());
        synthetic.is_synthetic = true;

        assert!(convert_method_symbol(&bridge, "sample.Service").is_none());
        assert!(convert_method_symbol(&synthetic, "sample.Service").is_none());
    }

    #[test]
    fn dependency_overloads_and_constructors_survive_index_insertion() {
        let class = ClassSymbols {
            fqn: "sample.Service".to_string(),
            package: Some("sample".to_string()),
            super_class: Some("java.lang.Object".to_string()),
            interfaces: Vec::new(),
            methods: vec![
                synthetic_bytecode_method(
                    "accept",
                    vec![DescriptorType::Class {
                        name: "java.util.Date".to_string(),
                    }],
                ),
                synthetic_bytecode_method(
                    "accept",
                    vec![DescriptorType::Class {
                        name: "java.sql.Date".to_string(),
                    }],
                ),
                synthetic_bytecode_method("<init>", vec![DescriptorType::Int]),
            ],
            fields: Vec::new(),
            type_parameters: Vec::new(),
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };
        let index = crate::WorkspaceIndex::new(std::path::PathBuf::from("/tmp/test"));
        index.add_external_symbols_batch(convert_java_class_symbols(&class));

        assert!(index
            .get_symbol("sample.Service.accept(java.util.Date)")
            .is_some());
        assert!(index
            .get_symbol("sample.Service.accept(java.sql.Date)")
            .is_some());
        assert_eq!(
            index
                .get_constructors_for_class("sample.Service")
                .into_iter()
                .map(|symbol| symbol.fqn.to_string())
                .collect::<Vec<_>>(),
            vec!["sample.Service.Service(int)"]
        );
    }

    #[test]
    fn nested_classes_are_indexed_by_source_fqn_and_simple_name() {
        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let source_root = temp.path().join("src/sample");
        let classes_root = temp.path().join("classes");
        std::fs::create_dir_all(&source_root).expect("create source directory");
        std::fs::create_dir_all(&classes_root).expect("create classes directory");
        let source_path = source_root.join("Outer.java");
        std::fs::write(
            &source_path,
            r#"
                package sample;

                public class Outer {
                    public interface Contract {}
                    public static class Inner {}

                    public Inner make() { return new Inner(); }
                    public void accept(Inner value) {}

                    public void generatedClasses() {
                        class Local {
                            class NestedInsideLocal {}
                        }
                        Runnable anonymous = new Runnable() {
                            public void run() {}
                        };
                        new Local();
                        anonymous.run();
                    }
                }

                class Child extends Outer.Inner {}
                class Implementor implements Outer.Contract {}
            "#,
        )
        .expect("write Java fixture");

        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_root)
            .arg(&source_path)
            .output()
            .expect("run javac for nested-class fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let jar_path = temp.path().join("nested.jar");
        let jar_file = std::fs::File::create(&jar_path).expect("create fixture JAR");
        let mut jar = zip::ZipWriter::new(jar_file);
        let options = zip::write::SimpleFileOptions::default();
        let package_dir = classes_root.join("sample");
        let mut class_files = std::fs::read_dir(&package_dir)
            .expect("read compiled classes")
            .map(|entry| entry.expect("class file entry").path())
            .collect::<Vec<_>>();
        class_files.sort();
        for path in class_files {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("UTF-8 class filename");
            jar.start_file(format!("sample/{file_name}"), options)
                .expect("start class entry");
            jar.write_all(&std::fs::read(&path).expect("read class bytes"))
                .expect("write class entry");
        }
        jar.finish().expect("finish fixture JAR");

        let index = crate::WorkspaceIndex::new(temp.path().join("workspace"));
        index.index_jar_file(&jar_path).expect("index fixture JAR");

        let inner = index
            .get_symbol("sample.Outer.Inner")
            .expect("nested class must be findable by dotted FQN");
        assert_eq!(inner.simple_name.as_ref(), "Inner");
        let by_simple = index.find_by_simple_name("Inner");
        assert!(
            by_simple
                .iter()
                .any(|location| location.fqn.as_ref() == "sample.Outer.Inner"),
            "nested class must be findable by simple name: {by_simple:?}"
        );
        assert!(
            index.get_symbol("sample.Outer$Inner").is_none(),
            "binary FQN must not leak into the source index"
        );

        assert_eq!(
            index
                .get_symbol("sample.Child")
                .and_then(|symbol| symbol.parent_class.clone())
                .as_deref(),
            Some("sample.Outer.Inner"),
            "nested superclass links must use source form"
        );
        assert!(
            index
                .get_symbol("sample.Implementor")
                .is_some_and(|symbol| symbol
                    .interfaces
                    .iter()
                    .any(|name| name.as_ref() == "sample.Outer.Contract")),
            "nested interface links must use source form"
        );
        assert_eq!(
            index
                .get_symbol("sample.Outer.make")
                .and_then(|symbol| symbol.return_type.clone())
                .as_deref(),
            Some("sample.Outer.Inner"),
            "descriptor-derived nested return types must retain canonical source identity"
        );

        let external = index.get_external_symbols();
        assert!(
            external.iter().all(|symbol| {
                !symbol.fqn.contains("$1")
                    && symbol.simple_name.as_ref() != "Local"
                    && symbol.simple_name.as_ref() != "NestedInsideLocal"
            }),
            "anonymous/local classes must not enter the external source index"
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_archive_snapshot_survives_alias_retarget_and_backing_replacement() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let first = compile_test_jar(temp.path(), "First");
        let second = compile_test_jar(temp.path(), "Second");
        let alias = temp.path().join("current.jar");
        symlink(&first, &alias).expect("create archive alias");

        let archive = crate::ExternalSourceArchive::capture(&alias);
        let index = crate::WorkspaceIndex::new(temp.path().join("workspace"));
        let ticket = index.begin_external_source_refresh("snapshot-test");
        let refresh = index
            .replace_external_archives_for_refresh(&ticket, std::slice::from_ref(&archive))
            .expect("publish captured archive plan");
        let revision = refresh
            .revision_for_identity(archive.identity())
            .expect("reserve captured archive revision");

        std::fs::remove_file(&alias).expect("remove original alias");
        symlink(&second, &alias).expect("retarget archive alias");
        let snapshot = archive
            .snapshot(8 * 1024 * 1024)
            .expect("snapshot captured canonical target");
        std::fs::copy(&second, &first).expect("replace captured target after snapshot");

        let stats = index
            .index_jar_snapshot_for_revision(snapshot, revision)
            .expect("publish immutable snapshot");
        assert_eq!(stats.classes_indexed, 1);
        assert!(index.get_symbol("snapshot.First").is_some());
        assert!(
            index.get_symbol("snapshot.Second").is_none(),
            "neither alias retargeting nor backing-file replacement may change captured bytes"
        );
    }

    #[test]
    fn an_unparseable_class_entry_is_counted_and_the_rest_of_the_jar_is_indexed() {
        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let good_jar = compile_test_jar(temp.path(), "Survivor");
        let jar_path = temp.path().join("damaged.jar");
        let mut jar = zip::ZipWriter::new(std::fs::File::create(&jar_path).expect("create JAR"));
        jar.start_file(
            "snapshot/Broken.class",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start broken entry");
        jar.write_all(b"not bytecode").expect("write broken entry");
        let mut good = zip::ZipArchive::new(std::fs::File::open(&good_jar).expect("open JAR"))
            .expect("read good JAR");
        let mut survivor = good
            .by_name("snapshot/Survivor.class")
            .expect("good class entry");
        jar.start_file(
            "snapshot/Survivor.class",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start good entry");
        std::io::copy(&mut survivor, &mut jar).expect("copy good entry");
        jar.finish().expect("finish damaged JAR");

        let (symbols, stats) =
            extract_jar_symbols(&jar_path, FileId::new(3)).expect("extract damaged JAR");

        assert!(
            symbols
                .iter()
                .any(|symbol| symbol.fqn.as_ref() == "snapshot.Survivor"),
            "a valid class must be indexed beside a broken one"
        );
        assert_eq!(stats.classes_found, 2);
        assert_eq!(stats.entries_skipped, 1);
    }

    #[test]
    fn member_javadoc_is_attached_to_the_indexed_method() {
        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let source_dir = temp.path().join("src/snapshot");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&source_dir).expect("create Java source directory");
        std::fs::create_dir_all(&classes_dir).expect("create Java classes directory");
        let source_path = source_dir.join("Doc.java");
        std::fs::write(
            &source_path,
            "package snapshot; public class Doc { public int size() { return 0; } public int get(int i) { return i; } }",
        )
        .expect("write Java source fixture");
        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&source_path)
            .output()
            .expect("run javac for archive fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let binary_path = temp.path().join("doc.jar");
        let mut jar = zip::ZipWriter::new(std::fs::File::create(&binary_path).expect("create JAR"));
        jar.start_file(
            "snapshot/Doc.class",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start class entry");
        jar.write_all(&std::fs::read(classes_dir.join("snapshot/Doc.class")).expect("read class"))
            .expect("write class entry");
        jar.finish().expect("finish fixture JAR");

        let javadoc_path = temp.path().join("doc-javadoc.jar");
        let mut javadoc =
            zip::ZipWriter::new(std::fs::File::create(&javadoc_path).expect("create Javadoc JAR"));
        javadoc
            .start_file(
                "snapshot/Doc.html",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("start Javadoc entry");
        javadoc
            .write_all(
                br#"<html><body>
<section class="detail" id="size()"><div class="block">The number of elements.</div></section>
<section class="detail" id="get(int)"><div class="block">Gets the element at an index.</div></section>
</body></html>"#,
            )
            .expect("write Javadoc entry");
        javadoc.finish().expect("finish Javadoc JAR");

        let binary = crate::ArchiveSnapshot::capture(&binary_path, 8 * 1024 * 1024)
            .expect("capture binary archive");
        let docs = crate::ArchiveSnapshot::capture(&javadoc_path, 8 * 1024 * 1024)
            .expect("capture Javadoc archive");
        let (symbols, stats) =
            extract_jar_symbols_with_docs_from_snapshots(binary, docs, FileId::new(9))
                .expect("extract with Javadoc");

        assert!(stats.documentation_loaded);
        let documentation = |fqn: &str| {
            symbols
                .iter()
                .find(|symbol| symbol.fqn.as_ref() == fqn)
                .and_then(|symbol| symbol.documentation.as_deref().map(str::to_string))
        };
        assert_eq!(
            documentation("snapshot.Doc.size()").as_deref(),
            Some("The number of elements.")
        );
        assert_eq!(
            documentation("snapshot.Doc.get(int)").as_deref(),
            Some("Gets the element at an index.")
        );
    }

    #[test]
    fn corrupt_javadoc_snapshot_does_not_discard_binary_symbols() {
        let temp = tempfile::tempdir().expect("temporary fixture directory");
        let binary_path = compile_test_jar(temp.path(), "Documented");
        let javadoc_path = temp.path().join("corrupt-javadoc.jar");
        std::fs::write(&javadoc_path, b"not a zip archive").expect("write corrupt Javadoc");
        let binary = crate::ArchiveSnapshot::capture(&binary_path, 8 * 1024 * 1024)
            .expect("capture binary archive");
        let javadoc = crate::ArchiveSnapshot::capture(&javadoc_path, 8 * 1024 * 1024)
            .expect("capture corrupt Javadoc bytes");

        let (symbols, stats) =
            extract_jar_symbols_with_docs_from_snapshots(binary, javadoc, FileId::new(7))
                .expect("optional Javadoc failure must not fail binary extraction");

        assert!(
            symbols
                .iter()
                .any(|symbol| symbol.fqn.as_ref() == "snapshot.Documented"),
            "binary symbols must survive an invalid optional attachment"
        );
        assert!(!stats.documentation_loaded);
        assert!(stats.documentation_error.is_some());
    }

    #[test]
    fn matches_omitted_jvm_signature_by_synthesized_descriptor() {
        let function = KotlinFunctionInfo {
            name: "decorate".to_string(),
            parameters: vec![KotlinParameterInfo {
                name: "count".to_string(),
                ty: kotlin_class_type("kotlin.Int"),
                vararg_element_type: None,
                has_default: false,
            }],
            return_type: kotlin_class_type("kotlin.String"),
            type_parameters: Vec::new(),
            extension_receiver_type: Some(kotlin_class_type("kotlin.String")),
            is_suspend: false,
            is_operator: false,
            is_inline: false,
            jvm_signature: Some(KotlinJvmMethodSignature {
                name: "decorate".to_string(),
                descriptor: None,
            }),
        };
        let class = ClassSymbols {
            fqn: "sample.SampleKt".to_string(),
            package: Some("sample".to_string()),
            super_class: Some("java.lang.Object".to_string()),
            interfaces: Vec::new(),
            methods: vec![MethodSymbol {
                name: "decorate".to_string(),
                parameters: vec![
                    DescriptorType::Class {
                        name: "java.lang.String".to_string(),
                    },
                    DescriptorType::Int,
                ],
                parameter_names: vec![None, None],
                return_type: DescriptorType::Class {
                    name: "java.lang.String".to_string(),
                },
                generic_return_type: None,
                generic_parameter_types: Vec::new(),
                type_parameters: Vec::new(),
                exceptions: Vec::new(),
                is_public: true,
                is_private: false,
                is_protected: false,
                is_static: true,
                is_final: true,
                is_abstract: false,
                is_native: false,
                is_bridge: false,
                is_synthetic: false,
                is_varargs: false,
                is_deprecated: false,
            }],
            fields: Vec::new(),
            type_parameters: Vec::new(),
            is_interface: false,
            is_abstract: false,
            is_final: true,
            is_public: true,
            is_deprecated: false,
        };
        let synthesized = synthesized_function_descriptor(&function, &[]);
        assert_eq!(
            synthesized.as_deref(),
            Some("(Ljava/lang/String;I)Ljava/lang/String;")
        );
        assert_eq!(
            find_method(
                &class,
                "decorate",
                function.jvm_signature.as_ref(),
                synthesized.as_deref(),
                2,
            ),
            Some(0)
        );
    }

    #[test]
    fn synthesizes_suspend_continuation_and_erased_return() {
        let function = KotlinFunctionInfo {
            name: "pause".to_string(),
            parameters: vec![KotlinParameterInfo {
                name: "millis".to_string(),
                ty: kotlin_class_type("kotlin.Long"),
                vararg_element_type: None,
                has_default: false,
            }],
            return_type: kotlin_class_type("kotlin.Unit"),
            type_parameters: Vec::new(),
            extension_receiver_type: None,
            is_suspend: true,
            is_operator: false,
            is_inline: false,
            jvm_signature: None,
        };
        assert_eq!(
            synthesized_function_descriptor(&function, &[]).as_deref(),
            Some("(JLkotlin/coroutines/Continuation;)Ljava/lang/Object;")
        );
    }
}
