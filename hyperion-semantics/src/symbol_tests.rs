// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Additional comprehensive tests for the symbol module
//!
//! These tests extend the basic tests in symbol.rs with more comprehensive coverage
//! for edge cases, complex scenarios, and error conditions.

#[cfg(test)]
mod symbol_comprehensive_tests {
    use crate::symbol::*;
    use std::collections::HashMap;

    // ========== Symbol Table Tests ==========

    #[test]
    fn test_deeply_nested_scopes() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();

        // Create deeply nested scope hierarchy: Package -> Class -> Method -> Block -> Block
        let package_scope = table.create_scope(ScopeKind::Package, Some(root));
        let class_scope = table.create_scope(ScopeKind::Class, Some(package_scope));
        let method_scope = table.create_scope(ScopeKind::Method, Some(class_scope));
        let block1_scope = table.create_scope(ScopeKind::Block, Some(method_scope));
        let block2_scope = table.create_scope(ScopeKind::Block, Some(block1_scope));

        // Add symbols at different levels
        let package_sym = Symbol::new(
            "PackageVar".into(),
            SymbolKind::Package,
            Type::Unknown,
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 10),
        );
        table.add_symbol(package_sym, package_scope);

        let class_field = Symbol::new(
            "classField".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 10, 20),
        );
        table.add_symbol(class_field, class_scope);

        let method_param = Symbol::new(
            "param".into(),
            SymbolKind::Parameter {
                param_type: Type::Class(ClassType::string()),
                is_varargs: false,
            },
            Type::Class(ClassType::string()),
            Visibility::Public,
            SourceLocation::new(FileId(0), 20, 30),
        );
        table.add_symbol(method_param, method_scope);

        let local_var = Symbol::new(
            "localVar".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Double),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Double),
            Visibility::Public,
            SourceLocation::new(FileId(0), 30, 40),
        );
        table.add_symbol(local_var, block2_scope);

        // Verify all symbols are accessible from deepest scope
        assert!(table.lookup_symbol("PackageVar", block2_scope).is_some());
        assert!(table.lookup_symbol("classField", block2_scope).is_some());
        assert!(table.lookup_symbol("param", block2_scope).is_some());
        assert!(table.lookup_symbol("localVar", block2_scope).is_some());

        // Verify scoping: localVar not accessible from method scope
        assert!(table.lookup_symbol("localVar", method_scope).is_none());
    }

    #[test]
    fn test_symbol_shadowing_multiple_levels() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let class_scope = table.create_scope(ScopeKind::Class, Some(root));
        let method_scope = table.create_scope(ScopeKind::Method, Some(class_scope));
        let block_scope = table.create_scope(ScopeKind::Block, Some(method_scope));

        // Define 'x' at each level with different types
        let field_x = Symbol::new(
            "x".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 0, 5),
        );
        table.add_symbol(field_x, class_scope);

        let param_x = Symbol::new(
            "x".into(),
            SymbolKind::Parameter {
                param_type: Type::Primitive(PrimitiveType::Long),
                is_varargs: false,
            },
            Type::Primitive(PrimitiveType::Long),
            Visibility::Public,
            SourceLocation::new(FileId(0), 10, 15),
        );
        table.add_symbol(param_x, method_scope);

        let local_x = Symbol::new(
            "x".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Double),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Double),
            Visibility::Public,
            SourceLocation::new(FileId(0), 20, 25),
        );
        let local_x_id = table.add_symbol(local_x, block_scope);

        // From block scope, should find the local variable (most recent shadow)
        let found = table.lookup_symbol("x", block_scope).unwrap();
        assert_eq!(found, local_x_id);
        let found_sym = table.get_symbol(found).unwrap();
        assert_eq!(found_sym.type_info, Type::Primitive(PrimitiveType::Double));
    }

    #[test]
    fn test_multiple_symbols_same_scope() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();

        // Add multiple symbols to the same scope
        for i in 0..100 {
            let symbol = Symbol::new(
                format!("var{}", i),
                SymbolKind::LocalVariable {
                    var_type: Type::Primitive(PrimitiveType::Int),
                    is_final: false,
                },
                Type::Primitive(PrimitiveType::Int),
                Visibility::Public,
                SourceLocation::new(FileId(0), i * 10, i * 10 + 5),
            );
            table.add_symbol(symbol, root);
        }

        // Verify all symbols can be found
        for i in 0..100 {
            assert!(table.lookup_symbol(&format!("var{}", i), root).is_some());
        }

        let stats = table.statistics();
        assert_eq!(stats.symbol_count, 100);
    }

    #[test]
    fn test_scope_parent_child_relationships() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let child1 = table.create_scope(ScopeKind::Class, Some(root));
        let child2 = table.create_scope(ScopeKind::Class, Some(root));
        let grandchild = table.create_scope(ScopeKind::Method, Some(child1));

        // Verify parent relationships
        assert_eq!(table.get_scope(child1).unwrap().parent, Some(root));
        assert_eq!(table.get_scope(child2).unwrap().parent, Some(root));
        assert_eq!(table.get_scope(grandchild).unwrap().parent, Some(child1));

        // Verify root has two children
        let root_scope = table.get_scope(root).unwrap();
        assert_eq!(root_scope.children.len(), 2);
        assert!(root_scope.children.contains(&child1));
        assert!(root_scope.children.contains(&child2));

        // Verify child1 has one child
        let child1_scope = table.get_scope(child1).unwrap();
        assert_eq!(child1_scope.children.len(), 1);
        assert!(child1_scope.children.contains(&grandchild));
    }

    #[test]
    fn test_local_symbols_vs_inherited() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let child = table.create_scope(ScopeKind::Class, Some(root));

        let root_sym = Symbol::new(
            "rootVar".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        table.add_symbol(root_sym, root);

        let child_sym = Symbol::new(
            "childVar".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 10, 15),
        );
        table.add_symbol(child_sym, child);

        // local_symbols should only return symbols defined directly in the scope
        let local = table.local_symbols(child);
        assert_eq!(local.len(), 1);
        assert_eq!(&*local[0].0, "childVar");

        // symbols_in_scope should return all accessible symbols
        let all = table.symbols_in_scope(child);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_find_enclosing_scopes() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let package = table.create_scope(ScopeKind::Package, Some(root));
        let class = table.create_scope(ScopeKind::Class, Some(package));
        let method = table.create_scope(ScopeKind::Method, Some(class));
        let block = table.create_scope(ScopeKind::Block, Some(method));

        // From deepest scope, find enclosing scopes
        assert_eq!(table.find_enclosing_class(block), Some(class));
        assert_eq!(table.find_enclosing_method(block), Some(method));
        assert_eq!(table.find_enclosing_package(block), Some(package));

        // From method, find enclosing class
        assert_eq!(table.find_enclosing_class(method), Some(class));

        // From class, no enclosing method
        assert_eq!(table.find_enclosing_method(class), None);
    }

    // Note: Cache invalidation test removed - resolution_cache is now in SymbolResolver,
    // not SymbolTable. See resolver_tests.rs for cache-related tests.

    // ========== Type Tests ==========

    #[test]
    fn test_primitive_type_widening_comprehensive() {
        use PrimitiveType::*;

        // Test all valid widening conversions
        assert!(Short.is_widening_from(Byte));
        assert!(Int.is_widening_from(Byte));
        assert!(Int.is_widening_from(Short));
        assert!(Int.is_widening_from(Char));
        assert!(Long.is_widening_from(Byte));
        assert!(Long.is_widening_from(Short));
        assert!(Long.is_widening_from(Int));
        assert!(Long.is_widening_from(Char));
        assert!(Float.is_widening_from(Byte));
        assert!(Float.is_widening_from(Short));
        assert!(Float.is_widening_from(Int));
        assert!(Float.is_widening_from(Long));
        assert!(Float.is_widening_from(Char));
        assert!(Double.is_widening_from(Byte));
        assert!(Double.is_widening_from(Short));
        assert!(Double.is_widening_from(Int));
        assert!(Double.is_widening_from(Long));
        assert!(Double.is_widening_from(Char));
        assert!(Double.is_widening_from(Float));

        // Test invalid conversions
        assert!(!Byte.is_widening_from(Short));
        assert!(!Int.is_widening_from(Long));
        assert!(!Float.is_widening_from(Double));
        assert!(!Boolean.is_widening_from(Int));
        assert!(!Int.is_widening_from(Boolean));
    }

    #[test]
    fn test_type_erasure_comprehensive() {
        // Generic class type
        let generic_list = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));

        let erased = generic_list.erasure();
        match erased {
            Type::Class(c) => {
                assert_eq!(&*c.name, "List");
                assert_eq!(c.package.as_deref(), Some("java.util".into()));
                assert!(c.type_arguments.is_empty());
            }
            _ => panic!("Expected class type"),
        }

        // Array of generic type
        let generic_array = Type::Array {
            element_type: Box::new(generic_list.clone()),
            dimensions: 2,
        };

        let erased_array = generic_array.erasure();
        match erased_array {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 2);
                match *element_type {
                    Type::Class(c) => assert!(c.type_arguments.is_empty()),
                    _ => panic!("Expected class type"),
                }
            }
            _ => panic!("Expected array type"),
        }

        // Type variable erasure to first bound
        let type_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType::string())],
        };

        let erased_var = type_var.erasure();
        assert_eq!(erased_var, Type::Class(ClassType::string()));

        // Type variable with no bounds erases to Object
        let unbounded_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let erased_unbounded = unbounded_var.erasure();
        match erased_unbounded {
            Type::Class(c) => assert_eq!(c.qualified_name(), "java.lang.Object"),
            _ => panic!("Expected Object type"),
        }
    }

    #[test]
    fn test_complex_generic_types() {
        // Map<String, List<Integer>>
        let inner_list = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::new(
                "Integer".into(),
                Some("java.lang".into()),
            ))],
        ));

        let map = Type::Class(ClassType::with_type_args(
            "Map".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string()), inner_list],
        ));

        match map {
            Type::Class(c) => {
                assert_eq!(&*c.name, "Map");
                assert_eq!(c.type_arguments.len(), 2);
            }
            _ => panic!("Expected class type"),
        }
    }

    #[test]
    fn test_wildcard_types() {
        // ? extends Number
        let extends_wildcard = Type::Wildcard {
            upper_bound: Some(Box::new(Type::Class(ClassType::new(
                "Number".into(),
                Some("java.lang".into()),
            )))),
            lower_bound: None,
        };

        // ? super Integer
        let super_wildcard = Type::Wildcard {
            upper_bound: None,
            lower_bound: Some(Box::new(Type::Class(ClassType::new(
                "Integer".into(),
                Some("java.lang".into()),
            )))),
        };

        // Unbounded wildcard ?
        let unbounded = Type::Wildcard {
            upper_bound: None,
            lower_bound: None,
        };

        assert_eq!(
            format!("{}", extends_wildcard),
            "? extends java.lang.Number"
        );
        assert_eq!(format!("{}", super_wildcard), "? super java.lang.Integer");
        assert_eq!(format!("{}", unbounded), "?");
    }

    #[test]
    fn test_intersection_types() {
        // T extends Comparable & Serializable
        let intersection = Type::Intersection {
            types: vec![
                Type::Class(ClassType::new(
                    "Comparable".into(),
                    Some("java.lang".into()),
                )),
                Type::Class(ClassType::new(
                    "Serializable".into(),
                    Some("java.io".into()),
                )),
            ],
        };

        let display = format!("{}", intersection);
        assert!(display.contains("Comparable"));
        assert!(display.contains("Serializable"));
        assert!(display.contains(" & "));
    }

    #[test]
    fn test_union_types() {
        // For multi-catch: catch(IOException | SQLException e)
        let union = Type::Union {
            types: vec![
                Type::Class(ClassType::new("IOException".into(), Some("java.io".into()))),
                Type::Class(ClassType::new(
                    "SQLException".into(),
                    Some("java.sql".into()),
                )),
            ],
        };

        let display = format!("{}", union);
        assert!(display.contains("IOException"));
        assert!(display.contains("SQLException"));
        assert!(display.contains(" | "));
    }

    #[test]
    fn test_array_types_multidimensional() {
        // int[]
        let array_1d = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        assert_eq!(format!("{}", array_1d), "int[]");

        // int[][]
        let array_2d = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 2,
        };
        assert_eq!(format!("{}", array_2d), "int[][]");

        // String[][][]
        let array_3d = Type::Array {
            element_type: Box::new(Type::Class(ClassType::string())),
            dimensions: 3,
        };
        assert_eq!(format!("{}", array_3d), "java.lang.String[][][]");
    }

    #[test]
    fn test_type_assignability_complex() {
        // Test array covariance: Object is assignable from any array
        let object = Type::Class(ClassType::object());
        let string_array = Type::Array {
            element_type: Box::new(Type::Class(ClassType::string())),
            dimensions: 1,
        };
        assert!(object.is_assignable_from(&string_array));

        // Test null assignability to arrays
        let null = Type::Null;
        assert!(string_array.is_assignable_from(&null));

        // Test primitive array assignability
        let int_array = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        let long_array = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Long)),
            dimensions: 1,
        };
        // Arrays are not covariant for primitive types
        assert!(!long_array.is_assignable_from(&int_array));

        // Test same array types
        let int_array2 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        assert!(int_array.is_assignable_from(&int_array2));
    }

    // ========== Modifier Tests ==========

    #[test]
    fn test_modifiers_comprehensive() {
        let mut mods = Modifiers::empty();

        // Test all modifiers
        mods.set(Modifiers::STATIC);
        assert!(mods.is_static());
        assert!(mods.has(Modifiers::STATIC));

        mods.set(Modifiers::FINAL);
        assert!(mods.is_final());
        assert!(mods.has(Modifiers::FINAL));

        mods.set(Modifiers::ABSTRACT);
        assert!(mods.is_abstract());
        assert!(mods.has(Modifiers::ABSTRACT));

        mods.set(Modifiers::SYNCHRONIZED);
        assert!(mods.has(Modifiers::SYNCHRONIZED));

        mods.set(Modifiers::NATIVE);
        assert!(mods.has(Modifiers::NATIVE));

        mods.set(Modifiers::STRICTFP);
        assert!(mods.has(Modifiers::STRICTFP));

        mods.set(Modifiers::TRANSIENT);
        assert!(mods.has(Modifiers::TRANSIENT));

        mods.set(Modifiers::VOLATILE);
        assert!(mods.has(Modifiers::VOLATILE));

        mods.set(Modifiers::DEFAULT);
        assert!(mods.has(Modifiers::DEFAULT));

        mods.set(Modifiers::SEALED);
        assert!(mods.has(Modifiers::SEALED));

        mods.set(Modifiers::NON_SEALED);
        assert!(mods.has(Modifiers::NON_SEALED));
    }

    #[test]
    fn test_modifiers_combinations() {
        // public static final
        let mut mods = Modifiers::empty();
        mods.set(Modifiers::STATIC);
        mods.set(Modifiers::FINAL);

        assert!(mods.is_static());
        assert!(mods.is_final());
        assert!(!mods.is_abstract());
    }

    // ========== SymbolKind Tests ==========

    #[test]
    fn test_symbol_kinds_comprehensive() {
        // Class with all variants
        let class_kinds = [
            SymbolKind::Class {
                is_interface: false,
                is_enum: false,
                is_record: false,
                is_annotation: false,
            },
            SymbolKind::Class {
                is_interface: true,
                is_enum: false,
                is_record: false,
                is_annotation: false,
            },
            SymbolKind::Class {
                is_interface: false,
                is_enum: true,
                is_record: false,
                is_annotation: false,
            },
            SymbolKind::Class {
                is_interface: false,
                is_enum: false,
                is_record: true,
                is_annotation: false,
            },
            SymbolKind::Class {
                is_interface: false,
                is_enum: false,
                is_record: false,
                is_annotation: true,
            },
        ];
        for (index, kind) in class_kinds.iter().enumerate() {
            let SymbolKind::Class {
                is_interface,
                is_enum,
                is_record,
                is_annotation,
            } = kind
            else {
                panic!("expected class kind");
            };
            let flags = [*is_interface, *is_enum, *is_record, *is_annotation];
            assert_eq!(
                flags.iter().filter(|&&flag| flag).count(),
                usize::from(index > 0)
            );
            if index > 0 {
                assert!(flags[index - 1]);
            }
        }

        // Method with parameters and type parameters
        let generic_method = SymbolKind::Method {
            is_constructor: false,
            parameters: vec![
                Parameter {
                    name: "arg1".into(),
                    param_type: Type::Primitive(PrimitiveType::Int),
                    is_varargs: false,
                },
                Parameter {
                    name: "args".into(),
                    param_type: Type::Class(ClassType::string()),
                    is_varargs: true,
                },
            ],
            return_type: Some(Type::Class(ClassType::string())),
            type_parameters: vec![TypeParameter {
                name: "T".into(),
                bounds: vec![Type::Class(ClassType::object())],
            }],
        };

        match generic_method {
            SymbolKind::Method {
                parameters,
                type_parameters,
                ..
            } => {
                assert_eq!(parameters.len(), 2);
                assert_eq!(type_parameters.len(), 1);
                assert!(parameters[1].is_varargs);
            }
            _ => panic!("Expected method kind"),
        }
    }

    // ========== AnnotationValue Tests ==========

    #[test]
    fn test_annotation_values() {
        let string_val = AnnotationValue::String("test".into());
        let nested = Annotation {
            name: "Inner".into(),
            arguments: HashMap::new(),
        };
        let values = [
            AnnotationValue::Integer(42),
            AnnotationValue::Boolean(true),
            AnnotationValue::Float(3.15),
            AnnotationValue::Class(Type::Class(ClassType::string())),
            AnnotationValue::Enum {
                enum_type: Type::Class(ClassType::new("Color".into(), Some("com.example".into()))),
                constant: "RED".into(),
            },
            AnnotationValue::Array(vec![
                AnnotationValue::Integer(1),
                AnnotationValue::Integer(2),
                AnnotationValue::Integer(3),
            ]),
            AnnotationValue::Annotation(Box::new(nested)),
        ];

        assert!(matches!(values[0], AnnotationValue::Integer(42)));
        assert!(matches!(values[1], AnnotationValue::Boolean(true)));
        assert!(matches!(
            values[2],
            AnnotationValue::Float(value) if value.to_bits() == 3.15_f64.to_bits()
        ));
        assert!(matches!(values[3], AnnotationValue::Class(_)));
        assert!(matches!(values[4], AnnotationValue::Enum { .. }));
        assert!(matches!(values[5], AnnotationValue::Array(ref entries) if entries.len() == 3));
        assert!(matches!(values[6], AnnotationValue::Annotation(_)));

        // Verify types compile (structure test)
        match string_val {
            AnnotationValue::String(s) => assert_eq!(&*s, "test"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_annotation_value_nan_equality() {
        // All NaN values should compare equal for Eq correctness
        let nan1 = AnnotationValue::Float(f64::NAN);
        let nan2 = AnnotationValue::Float(f64::NAN);

        // Same NaN compares equal
        assert_eq!(nan1, nan2);

        // Different NaN bit patterns should also compare equal
        // (f64::NAN has a specific bit pattern, but there are other valid NaN patterns)
        let nan3 = AnnotationValue::Float(f64::from_bits(0x7ff8_0000_0000_0001));
        assert_eq!(
            nan1, nan3,
            "Different NaN bit patterns should compare equal"
        );

        // But different non-NaN values are still different
        let normal1 = AnnotationValue::Float(1.0);
        let normal2 = AnnotationValue::Float(2.0);
        assert_ne!(normal1, normal2);

        // NaN is not equal to non-NaN
        assert_ne!(nan1, normal1);
    }

    // ========== Visibility Tests ==========

    #[test]
    fn test_visibility_levels() {
        assert_ne!(Visibility::Public, Visibility::Private);
        assert_ne!(Visibility::Protected, Visibility::PackagePrivate);

        // All visibility levels are distinct
        let visibilities = [
            Visibility::Public,
            Visibility::Protected,
            Visibility::PackagePrivate,
            Visibility::Private,
        ];

        for (i, v1) in visibilities.iter().enumerate() {
            for (j, v2) in visibilities.iter().enumerate() {
                if i == j {
                    assert_eq!(v1, v2);
                } else {
                    assert_ne!(v1, v2);
                }
            }
        }
    }

    // ========== Source Location Tests ==========

    #[test]
    fn test_source_location_creation() {
        let loc1 = SourceLocation::new(FileId(1), 10, 20);
        assert_eq!(loc1.file_id, FileId(1));
        assert_eq!(loc1.range, hyperion_types::TextRange::new(10, 20));
        assert_eq!(loc1.start_line, 0);
        assert_eq!(loc1.start_column, 0);

        let loc2 = SourceLocation::with_position(FileId(2), 10, 20, 5, 15);
        assert_eq!(loc2.file_id, FileId(2));
        assert_eq!(loc2.range, hyperion_types::TextRange::new(10, 20));
        assert_eq!(loc2.start_line, 5);
        assert_eq!(loc2.start_column, 15);
    }

    // ========== ClassType Tests ==========

    #[test]
    fn test_class_type_qualified_names() {
        let simple = ClassType::new("String".into(), None);
        assert_eq!(simple.qualified_name(), "String");

        let qualified = ClassType::new("String".into(), Some("java.lang".into()));
        assert_eq!(qualified.qualified_name(), "java.lang.String");

        let nested = ClassType::new("Entry".into(), Some("java.util.Map".into()));
        assert_eq!(nested.qualified_name(), "java.util.Map.Entry");
    }

    #[test]
    fn test_primitive_wrapper_classes() {
        use PrimitiveType::*;

        assert_eq!(Boolean.wrapper_class(), "Boolean");
        assert_eq!(Byte.wrapper_class(), "Byte");
        assert_eq!(Short.wrapper_class(), "Short");
        assert_eq!(Int.wrapper_class(), "Integer");
        assert_eq!(Long.wrapper_class(), "Long");
        assert_eq!(Char.wrapper_class(), "Character");
        assert_eq!(Float.wrapper_class(), "Float");
        assert_eq!(Double.wrapper_class(), "Double");
    }

    #[test]
    fn test_type_predicates() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        assert!(int_type.is_primitive());
        assert!(!int_type.is_reference());

        let string_type = Type::Class(ClassType::string());
        assert!(!string_type.is_primitive());
        assert!(string_type.is_reference());

        let array_type = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        assert!(!array_type.is_primitive());
        assert!(array_type.is_reference());

        let void_type = Type::Void;
        assert!(!void_type.is_primitive());
        assert!(!void_type.is_reference());

        let null_type = Type::Null;
        assert!(!null_type.is_primitive());
        assert!(null_type.is_reference());
    }
}
