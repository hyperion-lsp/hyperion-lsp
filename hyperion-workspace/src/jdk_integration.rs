// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK class integration
//!
//! This module converts JDK bytecode symbols into workspace symbols for indexing.

use hyperion_bytecode::extractor::ClassSymbols;
use hyperion_bytecode::extractor::{extract_generic_supertypes, extract_symbols};
use hyperion_bytecode::jdk_indexer::{find_java_home, get_modules_file, list_javabase_classes};
use hyperion_bytecode::parser::{get_source_class_name, parse_class_file};
use tracing::{debug, info, warn};

use crate::{Symbol, WorkspaceIndex};

/// Error type for JDK indexing operations
#[derive(Debug, thiserror::Error)]
pub enum JdkIndexError {
    #[error("JAVA_HOME not found")]
    JavaHomeNotFound,

    #[error("Modules file not found")]
    ModulesNotFound,

    #[error("JDK indexing error: {0}")]
    IndexingError(#[from] hyperion_bytecode::jdk_indexer::JdkError),

    #[error("Parse error: {0}")]
    ParseError(#[from] hyperion_bytecode::parser::ParseError),

    #[error("Extraction error: {0}")]
    ExtractionError(#[from] hyperion_bytecode::extractor::ExtractionError),
}

/// Convert bytecode class symbols to workspace symbols
///
/// Each class produces multiple symbols:
/// - One class symbol
/// - One symbol per method
/// - One symbol per field
pub fn convert_class_symbols(class: &ClassSymbols) -> Vec<Symbol> {
    crate::jar_indexer::convert_java_class_symbols(class)
}

/// Index JDK classes from java.base module into the workspace
///
/// This function:
/// 1. Discovers the JDK installation
/// 2. Lists all classes in java.base module
/// 3. Extracts and parses each class file
/// 4. Converts bytecode symbols to workspace symbols
/// 5. Adds symbols to the workspace index
///
/// Returns the number of classes successfully indexed.
pub fn index_jdk_classes(index: &WorkspaceIndex) -> Result<usize, JdkIndexError> {
    info!("Starting JDK class indexing");

    // Find JDK installation
    let java_home = find_java_home().map_err(|_| JdkIndexError::JavaHomeNotFound)?;
    debug!("Found JAVA_HOME: {}", java_home.display());

    // Get modules file
    let modules_file = get_modules_file(&java_home).map_err(|_| JdkIndexError::ModulesNotFound)?;
    debug!("Found modules file: {}", modules_file.display());

    // List and open java.base. The scoped image is removed as soon as this
    // indexing pass completes, including on early error returns.
    let class_names = list_javabase_classes(&modules_file)?;
    let jdk_image = hyperion_bytecode::jdk_indexer::JdkImage::open(&modules_file)?;
    info!("Found {} classes in java.base module", class_names.len());

    let mut indexed_count = 0;
    let mut all_symbols = Vec::new();
    let mut all_generic_supertypes = Vec::new();
    let total = class_names.len();

    for (i, class_name) in class_names.iter().enumerate() {
        // Progress logging every 500 classes
        if i > 0 && i % 500 == 0 {
            info!(
                "Indexed {}/{} JDK classes ({:.1}%)",
                i,
                total,
                i as f64 / total as f64 * 100.0
            );
        }

        // Extract class bytes
        let bytes = match jdk_image.extract_class(class_name) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to extract {}: {}", class_name, e);
                continue;
            }
        };

        // Parse class file
        let class_file = match parse_class_file(&bytes) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to parse {}: {}", class_name, e);
                continue;
            }
        };

        // Extract symbols
        let Some(source_fqn) = get_source_class_name(&class_file) else {
            debug!("Skipping anonymous/local JDK class {}", class_name);
            continue;
        };
        let generic_supertypes = extract_generic_supertypes(&class_file);

        let mut class_symbols = match extract_symbols(&class_file) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to extract symbols from {}: {}", class_name, e);
                continue;
            }
        };
        crate::jar_indexer::normalize_class_symbols_for_index(
            &mut class_symbols,
            source_fqn.clone(),
        );

        // Convert to workspace symbols
        let symbols = convert_class_symbols(&class_symbols);
        all_symbols.extend(symbols);
        if !generic_supertypes.is_empty() {
            all_generic_supertypes.push(crate::TypeGenericSupertypes {
                type_fqn: source_fqn,
                edges: generic_supertypes
                    .into_iter()
                    .map(|edge| crate::GenericSupertype {
                        written_target: edge.target,
                        arguments: edge.arguments,
                    })
                    .collect(),
            });
        }

        indexed_count += 1;
    }

    index.add_external_index_data_responsive(all_symbols, all_generic_supertypes);

    info!(
        "JDK indexing complete: {}/{} classes indexed",
        indexed_count, total
    );
    Ok(indexed_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_bytecode::descriptor::DescriptorType;
    use hyperion_bytecode::extractor::{ClassSymbols, MethodSymbol};
    use lsp_types::SymbolKind;

    #[test]
    fn test_convert_simple_class() {
        let class = ClassSymbols {
            fqn: "com.example.User".into(),
            package: Some("com.example".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let symbols = convert_class_symbols(&class);

        assert_eq!(symbols.len(), 1);
        let class_symbol = &symbols[0];
        assert_eq!(class_symbol.fqn.as_ref(), "com.example.User");
        assert_eq!(class_symbol.simple_name.as_ref(), "User");
        assert_eq!(class_symbol.kind, SymbolKind::CLASS);
        assert!(class_symbol.modifiers.is_public);
        assert_eq!(class_symbol.parent_class, Some("java.lang.Object".into()));
    }

    #[test]
    fn test_convert_class_preserves_type_parameters() {
        let class = ClassSymbols {
            fqn: "java.util.Map".into(),
            package: Some("java.util".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            type_parameters: vec!["K".into(), "V".into()],
            is_interface: true,
            is_abstract: true,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let symbols = convert_class_symbols(&class);
        assert_eq!(symbols[0].type_parameters.len(), 2);
        assert_eq!(symbols[0].type_parameters[0].name.as_ref(), "K");
        assert_eq!(symbols[0].type_parameters[0].position, 0);
        assert_eq!(symbols[0].type_parameters[1].name.as_ref(), "V");
        assert_eq!(symbols[0].type_parameters[1].position, 1);
    }

    #[test]
    fn test_convert_method_preserves_generic_contract() {
        let method = MethodSymbol {
            name: "identity".into(),
            parameters: vec![DescriptorType::Class {
                name: "java.lang.Object".into(),
            }],
            parameter_names: vec![Some("value".into())],
            return_type: DescriptorType::Class {
                name: "java.lang.Object".into(),
            },
            generic_return_type: Some("Result".into()),
            generic_parameter_types: vec!["Result".into()],
            type_parameters: vec!["Result".into()],
            exceptions: Vec::new(),
            is_public: true,
            is_private: false,
            is_protected: false,
            is_static: true,
            is_final: false,
            is_abstract: false,
            is_native: false,
            is_bridge: false,
            is_synthetic: false,
            is_varargs: false,
            is_deprecated: false,
        };

        let symbol = crate::jar_indexer::convert_method_symbol(&method, "com.example.Factory")
            .expect("method should convert");

        assert_eq!(symbol.parameters[0].as_ref(), "Result value");
        assert_eq!(symbol.return_type.as_deref(), Some("Result"));
        assert_eq!(symbol.type_parameters.len(), 1);
        assert_eq!(symbol.type_parameters[0].name.as_ref(), "Result");
    }

    #[test]
    fn test_convert_class_with_methods() {
        let class = ClassSymbols {
            fqn: "java.lang.String".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![
                MethodSymbol {
                    name: "length".into(),
                    parameters: vec![],
                    parameter_names: vec![],
                    return_type: DescriptorType::Int,
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
                    generic_return_type: None,
                    generic_parameter_types: Vec::new(),
                    type_parameters: Vec::new(),
                    is_deprecated: false,
                },
                MethodSymbol {
                    name: "substring".into(),
                    parameters: vec![DescriptorType::Int, DescriptorType::Int],
                    parameter_names: vec![Some("beginIndex".into()), Some("endIndex".into())],
                    return_type: DescriptorType::Class {
                        name: "java.lang.String".into(),
                    },
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
                    generic_return_type: None,
                    generic_parameter_types: Vec::new(),
                    type_parameters: Vec::new(),
                    is_deprecated: false,
                },
            ],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: true,
            is_public: true,
            is_deprecated: false,
        };

        let symbols = convert_class_symbols(&class);

        // 1 class + 2 methods = 3 symbols
        assert_eq!(symbols.len(), 3);

        // Check length() method
        let length_method = symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "length")
            .unwrap();
        assert_eq!(length_method.fqn.as_ref(), "java.lang.String.length()");
        assert_eq!(length_method.kind, SymbolKind::METHOD);
        assert_eq!(length_method.parameters.len(), 0);
        assert_eq!(length_method.return_type, Some("int".into()));
        assert_eq!(length_method.container, Some("java.lang.String".into()));

        // Check substring() method
        let substring_method = symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "substring")
            .unwrap();
        assert_eq!(
            substring_method.fqn.as_ref(),
            "java.lang.String.substring(int,int)"
        );
        assert_eq!(
            substring_method.parameters,
            vec!["int beginIndex".into(), "int endIndex".into()]
        );
        assert_eq!(
            substring_method.return_type,
            Some("java.lang.String".into())
        );
    }

    #[test]
    fn test_convert_interface() {
        let class = ClassSymbols {
            fqn: "java.util.List".into(),
            package: Some("java.util".into()),
            super_class: None,
            interfaces: vec![],
            methods: vec![MethodSymbol {
                name: "add".into(),
                parameters: vec![DescriptorType::Class {
                    name: "java.lang.Object".into(),
                }],
                parameter_names: vec![Some("e".into())],
                return_type: DescriptorType::Boolean,
                exceptions: Vec::new(),
                is_public: true,
                is_private: false,
                is_protected: false,
                is_static: false,
                is_final: false,
                is_abstract: true,
                is_native: false,
                is_bridge: false,
                is_synthetic: false,
                is_varargs: false,
                generic_return_type: None,
                generic_parameter_types: Vec::new(),
                type_parameters: Vec::new(),
                is_deprecated: false,
            }],
            fields: vec![],
            type_parameters: vec![],
            is_interface: true,
            is_abstract: true,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let symbols = convert_class_symbols(&class);

        assert_eq!(symbols.len(), 2); // interface + 1 method

        let interface_symbol = &symbols[0];
        assert_eq!(interface_symbol.kind, SymbolKind::INTERFACE);
        assert!(interface_symbol.modifiers.is_abstract);

        let add_method = &symbols[1];
        assert_eq!(add_method.simple_name.as_ref(), "add");
        assert_eq!(add_method.parameters, vec!["java.lang.Object e".into()]);
        assert_eq!(add_method.return_type, Some("boolean".into()));
    }

    #[test]
    fn test_inheritance_chain_symbols() {
        // Test the inheritance chain: ExecutionException → Exception → Throwable → Object

        // Object - no parent
        let object_class = ClassSymbols {
            fqn: "java.lang.Object".into(),
            package: Some("java.lang".into()),
            super_class: None, // Object has no parent
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let object_symbols = convert_class_symbols(&object_class);
        assert_eq!(object_symbols[0].parent_class, None);
        println!("OK Object has no parent");

        // Throwable extends Object
        let throwable_class = ClassSymbols {
            fqn: "java.lang.Throwable".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![MethodSymbol {
                name: "getMessage".into(),
                parameters: vec![],
                parameter_names: vec![],
                return_type: DescriptorType::Class {
                    name: "java.lang.String".into(),
                },
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
                generic_return_type: None,
                generic_parameter_types: Vec::new(),
                type_parameters: Vec::new(),
                is_deprecated: false,
            }],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let throwable_symbols = convert_class_symbols(&throwable_class);
        assert_eq!(
            throwable_symbols[0].parent_class,
            Some("java.lang.Object".into())
        );
        assert_eq!(throwable_symbols[0].fqn.as_ref(), "java.lang.Throwable");

        // Verify getMessage method
        let get_message = &throwable_symbols[1];
        assert_eq!(get_message.simple_name.as_ref(), "getMessage");
        assert_eq!(get_message.fqn.as_ref(), "java.lang.Throwable.getMessage()");
        assert_eq!(get_message.container, Some("java.lang.Throwable".into()));
        println!("OK Throwable extends Object, has getMessage()");

        // Exception extends Throwable
        let exception_class = ClassSymbols {
            fqn: "java.lang.Exception".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Throwable".into()),
            interfaces: vec![],
            methods: vec![], // Exception doesn't override getMessage
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let exception_symbols = convert_class_symbols(&exception_class);
        assert_eq!(
            exception_symbols[0].parent_class,
            Some("java.lang.Throwable".into())
        );
        println!("OK Exception extends Throwable");

        // ExecutionException extends Exception
        let exec_exception_class = ClassSymbols {
            fqn: "java.util.concurrent.ExecutionException".into(),
            package: Some("java.util.concurrent".into()),
            super_class: Some("java.lang.Exception".into()),
            interfaces: vec![],
            methods: vec![], // ExecutionException doesn't override getMessage
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let exec_exception_symbols = convert_class_symbols(&exec_exception_class);
        assert_eq!(
            exec_exception_symbols[0].parent_class,
            Some("java.lang.Exception".into())
        );
        println!("OK ExecutionException extends Exception");

        println!("\n📊 Full inheritance chain preserved:");
        println!("  ExecutionException.parent_class = Exception");
        println!("  Exception.parent_class = Throwable");
        println!("  Throwable.parent_class = Object");
        println!("  Object.parent_class = None");
    }

    #[test]
    fn test_method_container_relationship() {
        // Verify that methods are correctly linked to their containing class
        let class = ClassSymbols {
            fqn: "java.lang.Throwable".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![
                MethodSymbol {
                    name: "getMessage".into(),
                    parameters: vec![],
                    parameter_names: vec![],
                    return_type: DescriptorType::Class {
                        name: "java.lang.String".into(),
                    },
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
                    generic_return_type: None,
                    generic_parameter_types: Vec::new(),
                    type_parameters: Vec::new(),
                    is_deprecated: false,
                },
                MethodSymbol {
                    name: "getCause".into(),
                    parameters: vec![],
                    parameter_names: vec![],
                    return_type: DescriptorType::Class {
                        name: "java.lang.Throwable".into(),
                    },
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
                    generic_return_type: None,
                    generic_parameter_types: Vec::new(),
                    type_parameters: Vec::new(),
                    is_deprecated: false,
                },
            ],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        let symbols = convert_class_symbols(&class);

        // 1 class + 2 methods = 3 symbols
        assert_eq!(symbols.len(), 3);

        let class_symbol = &symbols[0];
        assert_eq!(class_symbol.fqn.as_ref(), "java.lang.Throwable");
        assert_eq!(class_symbol.container, None); // Classes have no container

        let get_message = &symbols[1];
        assert_eq!(get_message.fqn.as_ref(), "java.lang.Throwable.getMessage()");
        assert_eq!(get_message.container, Some("java.lang.Throwable".into()));
        assert_eq!(get_message.kind, SymbolKind::METHOD);

        let get_cause = &symbols[2];
        assert_eq!(get_cause.fqn.as_ref(), "java.lang.Throwable.getCause()");
        assert_eq!(get_cause.container, Some("java.lang.Throwable".into()));
        assert_eq!(get_cause.return_type, Some("java.lang.Throwable".into()));

        println!("OK Methods correctly linked to container class");
    }

    #[test]
    fn test_inheritance_aware_method_lookup() {
        use crate::index::WorkspaceIndex;
        use std::path::PathBuf;

        // Create workspace index
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

        // Add Object class (root of hierarchy)
        let object_class = ClassSymbols {
            fqn: "java.lang.Object".into(),
            package: Some("java.lang".into()),
            super_class: None,
            interfaces: vec![],
            methods: vec![MethodSymbol {
                name: "toString".into(),
                parameters: vec![],
                parameter_names: vec![],
                return_type: DescriptorType::Class {
                    name: "java.lang.String".into(),
                },
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
                generic_return_type: None,
                generic_parameter_types: Vec::new(),
                type_parameters: Vec::new(),
                is_deprecated: false,
            }],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        // Add Throwable class (extends Object, has getMessage)
        let throwable_class = ClassSymbols {
            fqn: "java.lang.Throwable".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![MethodSymbol {
                name: "getMessage".into(),
                parameters: vec![],
                parameter_names: vec![],
                return_type: DescriptorType::Class {
                    name: "java.lang.String".into(),
                },
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
                generic_return_type: None,
                generic_parameter_types: Vec::new(),
                type_parameters: Vec::new(),
                is_deprecated: false,
            }],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        // Add Exception class (extends Throwable, no new methods)
        let exception_class = ClassSymbols {
            fqn: "java.lang.Exception".into(),
            package: Some("java.lang".into()),
            super_class: Some("java.lang.Throwable".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        // Add ExecutionException (extends Exception)
        let exec_exception_class = ClassSymbols {
            fqn: "java.util.concurrent.ExecutionException".into(),
            package: Some("java.util.concurrent".into()),
            super_class: Some("java.lang.Exception".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            type_parameters: vec![],
            is_interface: false,
            is_abstract: false,
            is_final: false,
            is_public: true,
            is_deprecated: false,
        };

        // Convert and add all classes to the index
        for class in [
            &object_class,
            &throwable_class,
            &exception_class,
            &exec_exception_class,
        ] {
            let symbols = convert_class_symbols(class);
            for symbol in symbols {
                index.add_external_symbol(symbol);
            }
        }

        // Test 1: Direct method lookup (should work)
        let methods = index.find_methods_by_name("java.lang.Throwable", "getMessage");
        assert_eq!(
            methods.len(),
            1,
            "getMessage should be found directly on Throwable"
        );
        println!("OK Throwable.getMessage() found directly");

        // Test 2: Direct lookup on subclass (should NOT find method)
        let methods =
            index.find_methods_by_name("java.util.concurrent.ExecutionException", "getMessage");
        assert_eq!(
            methods.len(),
            0,
            "getMessage should NOT be found directly on ExecutionException"
        );
        println!(
            "OK ExecutionException.getMessage() not found directly (correct - it's inherited)"
        );

        // Test 3: Inheritance-aware lookup (should find method from parent)
        let methods = index.find_methods_by_name_with_inheritance(
            "java.util.concurrent.ExecutionException",
            "getMessage",
        );
        assert_eq!(
            methods.len(),
            1,
            "getMessage should be found via inheritance"
        );
        assert_eq!(methods[0].container, Some("java.lang.Throwable".into()));
        println!("OK ExecutionException.getMessage() found via inheritance chain!");

        // Test 4: toString should be found on any class (inherited from Object)
        let methods = index.find_methods_by_name_with_inheritance(
            "java.util.concurrent.ExecutionException",
            "toString",
        );
        assert_eq!(
            methods.len(),
            1,
            "toString should be found via Object inheritance"
        );
        assert_eq!(methods[0].container, Some("java.lang.Object".into()));
        println!("OK ExecutionException.toString() found from Object!");

        // Test 5: find_method_by_args_with_inheritance
        let method = index.find_method_by_args_with_inheritance(
            "java.util.concurrent.ExecutionException",
            "getMessage",
            0,
        );
        assert!(method.is_some(), "getMessage() with 0 args should be found");
        println!("OK find_method_by_args_with_inheritance works!");

        println!("\nSUCCESS Inheritance-aware method lookup working correctly!");
    }
}
