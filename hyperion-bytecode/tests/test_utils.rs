// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test utilities for compiling and loading Java bytecode
//!
//! Provides helper functions for:
//! - Creating temporary directories with Java sources
//! - Compiling Java code with javac
//! - Loading compiled class files
//! - Creating VM instances and invoking methods

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[allow(dead_code)]
pub(crate) fn assert_uncaught_exception<T: std::fmt::Debug>(
    result: &Result<T, hyperion_bytecode::interpreter::VMError>,
    expected_class: &str,
) {
    assert!(
        matches!(
            result,
            Err(hyperion_bytecode::interpreter::VMError::UncaughtException {
                exception_class,
                ..
            }) if exception_class == expected_class
        ),
        "expected uncaught {expected_class}, got {result:?}"
    );
}

/// Find all .java files in a directory recursively
fn find_java_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut java_files = Vec::new();

    if !dir.exists() {
        return Ok(java_files);
    }

    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "java" {
                    java_files.push(entry.path().to_path_buf());
                }
            }
        }
    }

    Ok(java_files)
}

/// Helper for creating and compiling Java test projects
pub struct JavaTestProject {
    temp_dir: TempDir,
    source_dir: PathBuf,
    output_dir: PathBuf,
}

impl JavaTestProject {
    /// Create a new Java test project with standard directory structure
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new().context("Failed to create temp directory")?;
        let source_dir = temp_dir.path().join("src");
        let output_dir = temp_dir.path().join("classes");

        fs::create_dir_all(&source_dir).context("Failed to create source directory")?;
        fs::create_dir_all(&output_dir).context("Failed to create output directory")?;

        Ok(Self {
            temp_dir,
            source_dir,
            output_dir,
        })
    }

    /// Write a Java source file to the project
    ///
    /// # Arguments
    /// * `package_path` - Package path (e.g., "com/example")
    /// * `class_name` - Class name without .java extension
    /// * `content` - Full Java source code
    pub fn write_source(
        &self,
        package_path: &str,
        class_name: &str,
        content: &str,
    ) -> Result<PathBuf> {
        let package_dir = self.source_dir.join(package_path);
        fs::create_dir_all(&package_dir).context("Failed to create package directory")?;

        let file_path = package_dir.join(format!("{}.java", class_name));
        fs::write(&file_path, content).context("Failed to write source file")?;

        Ok(file_path)
    }

    /// Compile all Java sources in the project
    ///
    /// Returns the output directory containing .class files
    pub fn compile(&self) -> Result<PathBuf> {
        // Find all .java files
        let java_files = find_java_files(&self.source_dir)?;

        if java_files.is_empty() {
            anyhow::bail!("No Java source files found in {:?}", self.source_dir);
        }

        let javac_result = Command::new("javac")
            .arg("-d")
            .arg(&self.output_dir)
            .arg("-sourcepath")
            .arg(&self.source_dir)
            .args(&java_files)
            .output()
            .context("Failed to execute javac")?;

        if !javac_result.status.success() {
            let stderr = String::from_utf8_lossy(&javac_result.stderr);
            anyhow::bail!("javac compilation failed:\n{}", stderr);
        }

        Ok(self.output_dir.clone())
    }

    /// Load compiled class bytes
    ///
    /// # Arguments
    /// * `class_path` - Fully qualified class name (e.g., "com.example.MyClass")
    pub fn load_class_bytes(&self, class_path: &str) -> Result<Vec<u8>> {
        let class_file = self
            .output_dir
            .join(class_path.replace('.', "/"))
            .with_extension("class");

        fs::read(&class_file)
            .with_context(|| format!("Failed to read class file: {:?}", class_file))
    }

    /// Get the project root directory
    #[allow(dead_code)]
    pub fn root_dir(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Get the source directory
    #[allow(dead_code)]
    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    /// Get the output directory
    #[allow(dead_code)]
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }
}

impl Default for JavaTestProject {
    fn default() -> Self {
        Self::new().expect("Failed to create default JavaTestProject")
    }
}

/// Compile a single Java class from source string
///
/// # Arguments
/// * `class_name` - Simple class name (e.g., "MyClass")
/// * `source` - Full Java source code
///
/// # Returns
/// Tuple of (class bytes, temp directory - keep alive to prevent cleanup)
pub fn compile_java_class(class_name: &str, source: &str) -> Result<(Vec<u8>, TempDir)> {
    let project = JavaTestProject::new()?;
    project.write_source("", class_name, source)?;
    project.compile()?;
    let bytes = project.load_class_bytes(class_name)?;
    Ok((bytes, project.temp_dir))
}

/// Compile multiple Java classes that may reference each other
///
/// # Arguments
/// * `sources` - List of (class_name, source_code) tuples
///
/// # Returns
/// Map of class_name -> class bytes, and temp directory
pub fn compile_java_classes(
    sources: &[(&str, &str)],
) -> Result<(std::collections::HashMap<String, Vec<u8>>, TempDir)> {
    let project = JavaTestProject::new()?;

    for (class_name, source) in sources {
        project.write_source("", class_name, source)?;
    }

    project.compile()?;

    let mut classes = std::collections::HashMap::new();
    for (class_name, _) in sources {
        let bytes = project.load_class_bytes(class_name)?;
        classes.insert(class_name.to_string(), bytes);
    }

    Ok((classes, project.temp_dir))
}

/// Compile Java class with package structure
///
/// # Arguments
/// * `package` - Package name (e.g., "com.example")
/// * `class_name` - Simple class name
/// * `source` - Full Java source code (must include package declaration)
pub fn compile_with_package(
    package: &str,
    class_name: &str,
    source: &str,
) -> Result<(Vec<u8>, TempDir)> {
    let project = JavaTestProject::new()?;
    let package_path = package.replace('.', "/");
    project.write_source(&package_path, class_name, source)?;
    project.compile()?;
    let fqn = format!("{}.{}", package, class_name);
    let bytes = project.load_class_bytes(&fqn)?;
    Ok((bytes, project.temp_dir))
}

#[cfg(test)]
#[allow(
    clippy::items_after_test_module,
    reason = "this shared integration-test module keeps the byte-patching helper beside its users"
)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_simple_class() {
        let source = r#"
            public class HelloWorld {
                public static void main(String[] args) {
                    System.out.println("Hello, World!");
                }
            }
        "#;

        let result = compile_java_class("HelloWorld", source);
        assert!(result.is_ok(), "Compilation should succeed");

        let (bytes, _temp) = result.unwrap();
        assert!(!bytes.is_empty(), "Class bytes should not be empty");
        assert_eq!(
            &bytes[0..4],
            &[0xCA, 0xFE, 0xBA, 0xBE],
            "Should start with Java magic number"
        );
    }

    #[test]
    fn test_compile_with_package() {
        let source = r#"
            package com.example;

            public class MyClass {
                private int value;

                public int getValue() {
                    return value;
                }
            }
        "#;

        let result = compile_with_package("com.example", "MyClass", source);
        assert!(result.is_ok(), "Compilation should succeed");

        let (bytes, _temp) = result.unwrap();
        assert!(!bytes.is_empty(), "Class bytes should not be empty");
    }

    #[test]
    fn test_compile_multiple_classes() {
        let sources = [
            (
                "Person",
                r#"
                public class Person {
                    private String name;
                    public Person(String name) { this.name = name; }
                    public String getName() { return name; }
                }
                "#,
            ),
            (
                "Main",
                r#"
                public class Main {
                    public static void main(String[] args) {
                        Person p = new Person("Alice");
                        System.out.println(p.getName());
                    }
                }
                "#,
            ),
        ];

        let result = compile_java_classes(&sources);
        assert!(result.is_ok(), "Compilation should succeed");

        let (classes, _temp) = result.unwrap();
        assert_eq!(classes.len(), 2, "Should have compiled 2 classes");
        assert!(
            classes.contains_key("Person"),
            "Should contain Person class"
        );
        assert!(classes.contains_key("Main"), "Should contain Main class");
    }

    #[test]
    fn test_java_test_project() {
        let project = JavaTestProject::new().unwrap();

        let source = r#"
            package test.pkg;

            public class TestClass {
                public int add(int a, int b) {
                    return a + b;
                }
            }
        "#;

        project
            .write_source("test/pkg", "TestClass", source)
            .unwrap();
        project.compile().unwrap();

        let bytes = project.load_class_bytes("test.pkg.TestClass").unwrap();
        assert!(!bytes.is_empty(), "Should load compiled class");
    }
}
#[allow(dead_code)]
pub(crate) fn patch_method_access_flags(
    mut class_bytes: Vec<u8>,
    method_name: &str,
    descriptor: &str,
    patch: impl FnOnce(u16) -> u16,
) -> Vec<u8> {
    fn read_u16(bytes: &[u8], offset: &mut usize) -> u16 {
        let value = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]);
        *offset += 2;
        value
    }

    fn read_u32(bytes: &[u8], offset: &mut usize) -> u32 {
        let value = u32::from_be_bytes([
            bytes[*offset],
            bytes[*offset + 1],
            bytes[*offset + 2],
            bytes[*offset + 3],
        ]);
        *offset += 4;
        value
    }

    fn skip_attributes(bytes: &[u8], offset: &mut usize, count: u16) {
        for _ in 0..count {
            *offset += 2; // attribute_name_index
            let length = read_u32(bytes, offset) as usize;
            *offset += length;
        }
    }

    assert_eq!(&class_bytes[..4], &[0xca, 0xfe, 0xba, 0xbe]);
    let mut offset = 8;
    let constant_pool_count = read_u16(&class_bytes, &mut offset) as usize;
    let mut utf8 = vec![None; constant_pool_count];
    let mut index = 1;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length = read_u16(&class_bytes, &mut offset) as usize;
                utf8[index] = Some(
                    String::from_utf8_lossy(&class_bytes[offset..offset + length]).into_owned(),
                );
                offset += length;
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }

    offset += 6; // class access_flags, this_class, super_class
    let interface_count = read_u16(&class_bytes, &mut offset) as usize;
    offset += interface_count * 2;
    let field_count = read_u16(&class_bytes, &mut offset);
    for _ in 0..field_count {
        offset += 6; // access_flags, name_index, descriptor_index
        let attribute_count = read_u16(&class_bytes, &mut offset);
        skip_attributes(&class_bytes, &mut offset, attribute_count);
    }

    let method_count = read_u16(&class_bytes, &mut offset);
    let mut patch = Some(patch);
    for _ in 0..method_count {
        let access_offset = offset;
        let access_flags = read_u16(&class_bytes, &mut offset);
        let name_index = read_u16(&class_bytes, &mut offset) as usize;
        let descriptor_index = read_u16(&class_bytes, &mut offset) as usize;
        let attribute_count = read_u16(&class_bytes, &mut offset);
        if utf8[name_index].as_deref() == Some(method_name)
            && utf8[descriptor_index].as_deref() == Some(descriptor)
        {
            let patched = patch.take().expect("method matched more than once")(access_flags);
            class_bytes[access_offset..access_offset + 2].copy_from_slice(&patched.to_be_bytes());
        }
        skip_attributes(&class_bytes, &mut offset, attribute_count);
    }
    assert!(
        patch.is_none(),
        "method {method_name}{descriptor} not found"
    );
    class_bytes
}

#[allow(dead_code)]
pub(crate) fn patch_field_access_flags(
    mut class_bytes: Vec<u8>,
    field_name: &str,
    descriptor: &str,
    patch: impl FnOnce(u16) -> u16,
) -> Vec<u8> {
    fn read_u16(bytes: &[u8], offset: &mut usize) -> u16 {
        let value = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]);
        *offset += 2;
        value
    }

    fn read_u32(bytes: &[u8], offset: &mut usize) -> u32 {
        let value = u32::from_be_bytes([
            bytes[*offset],
            bytes[*offset + 1],
            bytes[*offset + 2],
            bytes[*offset + 3],
        ]);
        *offset += 4;
        value
    }

    fn skip_attributes(bytes: &[u8], offset: &mut usize, count: u16) {
        for _ in 0..count {
            *offset += 2;
            let length = read_u32(bytes, offset) as usize;
            *offset += length;
        }
    }

    let mut offset = 8;
    let constant_pool_count = read_u16(&class_bytes, &mut offset) as usize;
    let mut utf8 = vec![None; constant_pool_count];
    let mut index = 1;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length = read_u16(&class_bytes, &mut offset) as usize;
                utf8[index] = Some(
                    String::from_utf8_lossy(&class_bytes[offset..offset + length]).into_owned(),
                );
                offset += length;
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }

    offset += 6;
    let interface_count = read_u16(&class_bytes, &mut offset) as usize;
    offset += interface_count * 2;
    let field_count = read_u16(&class_bytes, &mut offset);
    let mut patch = Some(patch);
    for _ in 0..field_count {
        let access_offset = offset;
        let access_flags = read_u16(&class_bytes, &mut offset);
        let name_index = read_u16(&class_bytes, &mut offset) as usize;
        let descriptor_index = read_u16(&class_bytes, &mut offset) as usize;
        let attribute_count = read_u16(&class_bytes, &mut offset);
        if utf8[name_index].as_deref() == Some(field_name)
            && utf8[descriptor_index].as_deref() == Some(descriptor)
        {
            let patched = patch.take().expect("field matched more than once")(access_flags);
            class_bytes[access_offset..access_offset + 2].copy_from_slice(&patched.to_be_bytes());
        }
        skip_attributes(&class_bytes, &mut offset, attribute_count);
    }
    assert!(patch.is_none(), "field {field_name}:{descriptor} not found");
    class_bytes
}

/// Add a `ConstantValue` attribute to an integer field using constants that
/// already exist in the class file's constant pool.
///
/// Keeping the constant pool unchanged makes this a small, targeted binary
/// evolution fixture: javac emits the field's `<clinit>` assignment, while the
/// patched class also has an earlier preparation-time value.
#[allow(dead_code)]
pub(crate) fn add_integer_constant_value(
    mut class_bytes: Vec<u8>,
    field_name: &str,
    descriptor: &str,
    value: i32,
) -> Vec<u8> {
    fn read_u16(bytes: &[u8], offset: &mut usize) -> u16 {
        let value = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]);
        *offset += 2;
        value
    }

    fn read_u32(bytes: &[u8], offset: &mut usize) -> u32 {
        let value = u32::from_be_bytes([
            bytes[*offset],
            bytes[*offset + 1],
            bytes[*offset + 2],
            bytes[*offset + 3],
        ]);
        *offset += 4;
        value
    }

    fn skip_attributes(bytes: &[u8], offset: &mut usize, count: u16) {
        for _ in 0..count {
            *offset += 2;
            let length = read_u32(bytes, offset) as usize;
            *offset += length;
        }
    }

    let mut offset = 8;
    let constant_pool_count = read_u16(&class_bytes, &mut offset) as usize;
    let mut utf8 = vec![None; constant_pool_count];
    let mut integer_constant_index = None;
    let mut index = 1;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length = read_u16(&class_bytes, &mut offset) as usize;
                utf8[index] = Some(
                    String::from_utf8_lossy(&class_bytes[offset..offset + length]).into_owned(),
                );
                offset += length;
            }
            3 => {
                let constant = i32::from_be_bytes([
                    class_bytes[offset],
                    class_bytes[offset + 1],
                    class_bytes[offset + 2],
                    class_bytes[offset + 3],
                ]);
                if constant == value {
                    integer_constant_index = Some(index as u16);
                }
                offset += 4;
            }
            4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }

    let constant_value_name_index = utf8
        .iter()
        .position(|entry| entry.as_deref() == Some("ConstantValue"))
        .map(|index| index as u16)
        .expect("class must already contain a ConstantValue attribute name");
    let integer_constant_index =
        integer_constant_index.expect("requested integer must already exist in constant pool");

    offset += 6;
    let interface_count = read_u16(&class_bytes, &mut offset) as usize;
    offset += interface_count * 2;
    let field_count = read_u16(&class_bytes, &mut offset);
    for _ in 0..field_count {
        offset += 2; // access_flags
        let name_index = read_u16(&class_bytes, &mut offset) as usize;
        let descriptor_index = read_u16(&class_bytes, &mut offset) as usize;
        let attribute_count_offset = offset;
        let attribute_count = read_u16(&class_bytes, &mut offset);
        let attribute_start = offset;
        skip_attributes(&class_bytes, &mut offset, attribute_count);

        if utf8[name_index].as_deref() == Some(field_name)
            && utf8[descriptor_index].as_deref() == Some(descriptor)
        {
            assert_eq!(
                offset, attribute_start,
                "target field must not already have attributes"
            );
            class_bytes[attribute_count_offset..attribute_count_offset + 2]
                .copy_from_slice(&(attribute_count + 1).to_be_bytes());
            let mut attribute = Vec::with_capacity(8);
            attribute.extend_from_slice(&constant_value_name_index.to_be_bytes());
            attribute.extend_from_slice(&2_u32.to_be_bytes());
            attribute.extend_from_slice(&integer_constant_index.to_be_bytes());
            class_bytes.splice(offset..offset, attribute);
            return class_bytes;
        }
    }

    panic!("field {field_name}:{descriptor} not found")
}

#[allow(dead_code)]
pub(crate) fn duplicate_constant_value_attribute(
    mut class_bytes: Vec<u8>,
    field_name: &str,
    descriptor: &str,
) -> Vec<u8> {
    fn read_u16(bytes: &[u8], offset: &mut usize) -> u16 {
        let value = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]);
        *offset += 2;
        value
    }

    fn read_u32(bytes: &[u8], offset: &mut usize) -> u32 {
        let value = u32::from_be_bytes([
            bytes[*offset],
            bytes[*offset + 1],
            bytes[*offset + 2],
            bytes[*offset + 3],
        ]);
        *offset += 4;
        value
    }

    let mut offset = 8;
    let constant_pool_count = read_u16(&class_bytes, &mut offset) as usize;
    let mut utf8 = vec![None; constant_pool_count];
    let mut index = 1;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length = read_u16(&class_bytes, &mut offset) as usize;
                utf8[index] = Some(
                    String::from_utf8_lossy(&class_bytes[offset..offset + length]).into_owned(),
                );
                offset += length;
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }

    offset += 6;
    let interface_count = read_u16(&class_bytes, &mut offset) as usize;
    offset += interface_count * 2;
    let field_count = read_u16(&class_bytes, &mut offset);
    for _ in 0..field_count {
        offset += 2;
        let name_index = read_u16(&class_bytes, &mut offset) as usize;
        let descriptor_index = read_u16(&class_bytes, &mut offset) as usize;
        let attribute_count_offset = offset;
        let attribute_count = read_u16(&class_bytes, &mut offset);
        let target = utf8[name_index].as_deref() == Some(field_name)
            && utf8[descriptor_index].as_deref() == Some(descriptor);
        let mut constant_value_bytes = None;
        for _ in 0..attribute_count {
            let attribute_start = offset;
            let attribute_name_index = read_u16(&class_bytes, &mut offset) as usize;
            let length = read_u32(&class_bytes, &mut offset) as usize;
            offset += length;
            if target && utf8[attribute_name_index].as_deref() == Some("ConstantValue") {
                constant_value_bytes = Some(class_bytes[attribute_start..offset].to_vec());
            }
        }
        if target {
            let duplicate = constant_value_bytes.expect("target field has no ConstantValue");
            class_bytes[attribute_count_offset..attribute_count_offset + 2]
                .copy_from_slice(&(attribute_count + 1).to_be_bytes());
            class_bytes.splice(offset..offset, duplicate);
            return class_bytes;
        }
    }
    panic!("field {field_name}:{descriptor} not found")
}

#[allow(dead_code)]
pub(crate) fn patch_super_class_index(mut class_bytes: Vec<u8>, new_index: u16) -> Vec<u8> {
    let mut offset = 8;
    let constant_pool_count = u16::from_be_bytes([class_bytes[offset], class_bytes[offset + 1]]);
    offset += 2;
    let mut index = 1;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length =
                    u16::from_be_bytes([class_bytes[offset], class_bytes[offset + 1]]) as usize;
                offset += 2 + length;
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }
    let super_class_offset = offset + 4;
    class_bytes[super_class_offset..super_class_offset + 2]
        .copy_from_slice(&new_index.to_be_bytes());
    class_bytes
}

#[allow(dead_code)]
pub(crate) fn patch_utf8_constant(mut class_bytes: Vec<u8>, old: &str, new: &str) -> Vec<u8> {
    let mut offset = 8;
    let constant_pool_count = u16::from_be_bytes([class_bytes[offset], class_bytes[offset + 1]]);
    offset += 2;
    let mut index = 1;
    let mut patched = 0;
    while index < constant_pool_count {
        let tag = class_bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length =
                    u16::from_be_bytes([class_bytes[offset], class_bytes[offset + 1]]) as usize;
                offset += 2;
                if &class_bytes[offset..offset + length] == old.as_bytes() {
                    let length_offset = offset - 2;
                    class_bytes[length_offset..length_offset + 2]
                        .copy_from_slice(&(new.len() as u16).to_be_bytes());
                    class_bytes.splice(offset..offset + length, new.bytes());
                    patched += 1;
                    offset += new.len();
                } else {
                    offset += length;
                }
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => offset += 2,
            9 | 10 | 11 | 12 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }
    assert!(patched > 0, "UTF-8 constant {old:?} not found");
    class_bytes
}

#[allow(dead_code)]
pub(crate) fn patch_method_ref_owner(
    bytes: Vec<u8>,
    old_owner: &str,
    method_name: &str,
    descriptor: &str,
    new_owner: &str,
) -> Vec<u8> {
    patch_method_reference(bytes, old_owner, method_name, descriptor, new_owner, None)
}

#[allow(dead_code)]
pub(crate) fn patch_method_ref_tag(
    bytes: Vec<u8>,
    owner: &str,
    method_name: &str,
    descriptor: &str,
    new_tag: u8,
) -> Vec<u8> {
    assert!(matches!(new_tag, 10 | 11));
    patch_method_reference(bytes, owner, method_name, descriptor, owner, Some(new_tag))
}

fn patch_method_reference(
    mut bytes: Vec<u8>,
    old_owner: &str,
    method_name: &str,
    descriptor: &str,
    new_owner: &str,
    new_tag: Option<u8>,
) -> Vec<u8> {
    let mut offset = 8;
    let cp_count = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
    offset += 2;
    let mut utf8: Vec<Option<String>> = vec![None; cp_count];
    let mut classes = vec![None; cp_count];
    let mut names_and_types = vec![None; cp_count];
    let mut method_refs = Vec::new();
    let mut index = 1;
    while index < cp_count {
        let tag = bytes[offset];
        offset += 1;
        match tag {
            1 => {
                let length = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
                offset += 2;
                utf8[index] = Some(String::from_utf8_lossy(&bytes[offset..offset + length]).into());
                offset += length;
            }
            7 => {
                classes[index] = Some(u16::from_be_bytes([bytes[offset], bytes[offset + 1]]));
                offset += 2;
            }
            10 | 11 => {
                let tag_offset = offset - 1;
                let entry_offset = offset;
                let class = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
                let nat = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]);
                method_refs.push((tag_offset, entry_offset, class, nat));
                offset += 4;
            }
            12 => {
                names_and_types[index] = Some((
                    u16::from_be_bytes([bytes[offset], bytes[offset + 1]]),
                    u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]),
                ));
                offset += 4;
            }
            3 | 4 => offset += 4,
            5 | 6 => {
                offset += 8;
                index += 1;
            }
            8 | 16 | 19 | 20 => offset += 2,
            9 | 17 | 18 => offset += 4,
            15 => offset += 3,
            other => panic!("unsupported constant-pool tag {other}"),
        }
        index += 1;
    }
    let class_name = |class_index: u16| {
        classes[class_index as usize].and_then(|utf8_index| utf8[utf8_index as usize].as_deref())
    };
    let new_class_index = (1..cp_count)
        .find(|index| class_name(*index as u16) == Some(new_owner))
        .expect("new Methodref owner Class constant missing") as u16;
    let (tag_offset, entry_offset, _, _) = method_refs
        .into_iter()
        .find(|(_, _, class_index, nat_index)| {
            let Some((name_index, descriptor_index)) = names_and_types[*nat_index as usize] else {
                return false;
            };
            class_name(*class_index) == Some(old_owner)
                && utf8[name_index as usize].as_deref() == Some(method_name)
                && utf8[descriptor_index as usize].as_deref() == Some(descriptor)
        })
        .expect("target Methodref missing");
    bytes[entry_offset..entry_offset + 2].copy_from_slice(&new_class_index.to_be_bytes());
    if let Some(tag) = new_tag {
        bytes[tag_offset] = tag;
    }
    bytes
}
