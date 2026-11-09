// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The JVM view of Kotlin declarations.
//!
//! Java code never sees Kotlin declarations as written. It sees what `kotlinc`
//! compiles them to:
//!
//! * top-level functions and properties live in a *file facade* class
//!   (`Square.kt` -> `SquareKt`, or the `@file:JvmName` name) as static members,
//!   extension functions taking their receiver as the first parameter;
//! * a property is a private field plus `getX()` / `setX(v)` / `isX()`
//!   accessors, unless it is `@JvmField` or `const`, which are plain fields;
//! * `companion object` members are reached through `Owner.Companion`, and
//!   `@JvmStatic` / `@JvmField` / `const` ones additionally as statics of
//!   `Owner`;
//! * `@JvmOverloads` adds one overload per dropped trailing default;
//! * enums have static `values()` / `valueOf(String)`.
//!
//! The source extractor records what was *written*; this module adds what a
//! Java caller can *call*. Every symbol produced here is synthetic, carries the
//! Java spelling of its types (`double`, `java.util.List<demo.Shape>`), points
//! at the Kotlin declaration it derives from through `jvm_origin`, and uses
//! Java-style `Owner.name(erased,param,types)` FQNs so overloads do not
//! collide.

use crate::symbol_extractor::{ImportMap, Modifiers, Symbol, SymbolExtractor, TextRange};
use crate::{SyntaxKind, SyntaxNode};
use hyperion_types::TypeParameterInfo;
use lsp_types::SymbolKind;
use rowan::NodeOrToken;

/// One annotation as written: `@JvmStatic`, `@file:JvmName("X")`.
#[derive(Debug, Clone)]
struct Annotation {
    name: String,
    first_string: Option<String>,
}

/// Modifier keywords and annotations written in front of a declaration.
#[derive(Debug, Clone, Default)]
struct Written {
    keywords: Vec<SyntaxKind>,
    annotations: Vec<Annotation>,
}

impl Written {
    fn has(&self, keyword: SyntaxKind) -> bool {
        self.keywords.contains(&keyword)
    }

    fn annotation(&self, name: &str) -> Option<&Annotation> {
        self.annotations
            .iter()
            .find(|annotation| annotation.name == name)
    }

    fn has_annotation(&self, name: &str) -> bool {
        self.annotation(name).is_some()
    }

    /// `private` members are invisible to Java. `internal` members are name
    /// mangled by `kotlinc` (`getFoo$module`), which no Java caller writes.
    fn hidden_from_java(&self) -> bool {
        self.has(SyntaxKind::PRIVATE_KW) || self.has(SyntaxKind::INTERNAL_KW)
    }

    fn protected(&self) -> bool {
        self.has(SyntaxKind::PROTECTED_KW)
    }

    /// The identifier after `const`, which the lexer does not know as keyword.
    fn is_const(&self, node: &SyntaxNode) -> bool {
        modifier_idents(node).iter().any(|ident| ident == "const")
    }
}

/// Text of identifier-shaped modifiers (`const`, `tailrec`, ...) in the
/// MODIFIERS nodes directly in front of `node`.
fn modifier_idents(node: &SyntaxNode) -> Vec<String> {
    modifier_nodes(node)
        .iter()
        .flat_map(|modifiers| {
            modifiers
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| token.text().to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The run of MODIFIERS nodes immediately preceding `node`.
fn modifier_nodes(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut nodes = Vec::new();
    let mut previous = node.prev_sibling();
    while let Some(sibling) = previous {
        if sibling.kind() != SyntaxKind::MODIFIERS {
            break;
        }
        previous = sibling.prev_sibling();
        nodes.push(sibling);
    }
    nodes
}

fn written_before(node: &SyntaxNode) -> Written {
    let mut written = Written::default();
    for modifiers in modifier_nodes(node).into_iter().rev() {
        collect_written(&modifiers, &mut written);
    }
    written
}

fn collect_written(modifiers: &SyntaxNode, written: &mut Written) {
    for element in modifiers.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => {
                if !token.kind().is_trivia() {
                    written.keywords.push(token.kind());
                }
            }
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::ANNOTATION => {
                written.annotations.push(read_annotation(&node));
            }
            NodeOrToken::Node(_) => {}
        }
    }
}

fn read_annotation(node: &SyntaxNode) -> Annotation {
    let name = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
        .last()
        .unwrap_or_default();
    let first_string = node
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| token.kind() == SyntaxKind::STRING_LITERAL)
        .map(|token| token.text().trim_matches('"').to_string());
    Annotation { name, first_string }
}

/// One parameter of a function or constructor as declared.
#[derive(Debug, Clone)]
struct Param {
    name: String,
    kotlin_type: String,
    has_default: bool,
    is_vararg: bool,
    /// `val` / `var` in a primary constructor
    property: Option<bool>,
    written: Written,
    name_range: TextRange,
}

struct View<'a> {
    package: Option<&'a str>,
    imports: &'a ImportMap,
    out: Vec<Symbol>,
}

/// Compute the JVM-only symbols of one Kotlin file, and the FQN of its file
/// facade among them when one is produced.
///
/// `kotlinc` emits the facade for top-level functions and properties, and also
/// for a file that declares only typealiases, since their metadata lives there
/// (the stdlib's `kotlin.collections.TypeAliasesKt` holds nothing else).
pub(crate) fn jvm_view_symbols(
    root: &SyntaxNode,
    package: Option<&str>,
    file_stem: Option<&str>,
    imports: &ImportMap,
) -> (Vec<Symbol>, Option<String>) {
    let mut view = View {
        package,
        imports,
        out: Vec::new(),
    };

    let mut facade_name = file_stem.map(facade_class_name);
    let mut multifile = false;
    for annotation in root
        .children()
        .filter(|child| child.kind() == SyntaxKind::ANNOTATION)
        .map(|node| read_annotation(&node))
        .chain(
            root.children()
                .filter(|child| child.kind() == SyntaxKind::MODIFIERS)
                .flat_map(|modifiers| {
                    let mut written = Written::default();
                    collect_written(&modifiers, &mut written);
                    written.annotations
                }),
        )
    {
        match annotation.name.as_str() {
            "JvmName" => {
                if let Some(name) = annotation.first_string {
                    facade_name = Some(name);
                }
            }
            "JvmMultifileClass" => multifile = true,
            _ => {}
        }
    }

    let facade_fqn = facade_name.as_ref().map(|name| view.qualify_top(name));
    let mut facade_members = 0usize;

    for node in root.children() {
        match node.kind() {
            SyntaxKind::FUNCTION_DECL => {
                if let Some(facade) = &facade_fqn {
                    facade_members += usize::from(view.function(&node, facade, Owner::Facade));
                }
            }
            SyntaxKind::PROPERTY_DECL => {
                if let Some(facade) = &facade_fqn {
                    facade_members += usize::from(view.property(&node, facade, Owner::Facade));
                }
            }
            SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL | SyntaxKind::OBJECT_DECL => {
                if let Some(fqn) = view.declared_fqn(&node, None) {
                    view.class_like(&node, &fqn, None);
                }
            }
            _ => {}
        }
    }

    let declares_type_alias = root
        .children()
        .any(|child| child.kind() == SyntaxKind::TYPEALIAS_DECL);
    let mut produced_facade = None;
    if let (Some(facade), Some(name)) = (facade_fqn, facade_name) {
        if (facade_members > 0 || declares_type_alias) && !multifile {
            produced_facade = Some(facade.clone());
            let range = root
                .children()
                .find(|child| {
                    matches!(
                        child.kind(),
                        SyntaxKind::FUNCTION_DECL
                            | SyntaxKind::PROPERTY_DECL
                            | SyntaxKind::TYPEALIAS_DECL
                    )
                })
                .map_or_else(
                    || TextRange::new(0, 0),
                    |child| {
                        let range = child.text_range();
                        TextRange::new(range.start().into(), range.start().into())
                    },
                );
            view.out.push(Symbol {
                fqn: facade.clone(),
                simple_name: name,
                range,
                kind: SymbolKind::CLASS,
                modifiers: Modifiers {
                    is_public: true,
                    is_final: true,
                    ..Modifiers::default()
                },
                container: package.map(str::to_string),
                return_type: None,
                parameters: Vec::new(),
                parent_class: None,
                interfaces: Vec::new(),
                documentation: Some("JVM file facade generated for top-level declarations".into()),
                custom_getter: false,
                custom_setter: false,
                getter_visibility: None,
                setter_visibility: None,
                type_parameters: Vec::new(),
                constructor_params: Vec::new(),
                is_synthetic: true,
                receiver_type: None,
                jvm_origin: Some(facade),
            });
        }
    }

    (view.out, produced_facade)
}

/// `Square.kt` -> `SquareKt`; characters that cannot appear in a class name
/// become `_` and the first letter is upper-cased, as `kotlinc` does.
fn facade_class_name(file_stem: &str) -> String {
    let mut name: String = file_stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Some(first) = name.chars().next() {
        let upper: String = first.to_uppercase().collect();
        name.replace_range(..first.len_utf8(), &upper);
    }
    format!("{name}Kt")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// Static members of the file facade.
    Facade,
    /// Instance members of a class (or interface).
    Class,
    /// Members of a non-companion `object`.
    Object,
    /// Members of a `companion object` (instance members of `Owner.Companion`).
    Companion,
}

impl View<'_> {
    /// FQN under which the *Kotlin* extractor records a member: top-level
    /// declarations live in the package, not in their file facade.
    fn origin_fqn(&self, container: &str, owner: Owner, name: &str) -> String {
        if owner == Owner::Facade {
            self.qualify_top(name)
        } else {
            format!("{container}.{name}")
        }
    }

    fn qualify_top(&self, simple: &str) -> String {
        self.package.map_or_else(
            || simple.to_string(),
            |package| format!("{package}.{simple}"),
        )
    }

    /// FQN of a class-like declaration nested in `outer` (or top level).
    fn declared_fqn(&self, node: &SyntaxNode, outer: Option<&str>) -> Option<String> {
        let name = class_like_name(node)?;
        Some(outer.map_or_else(
            || self.qualify_top(&name),
            |outer| format!("{outer}.{name}"),
        ))
    }

    fn class_like(&mut self, node: &SyntaxNode, fqn: &str, outer_of_companion: Option<&str>) {
        let written = written_before(node);
        let is_interface = node.kind() == SyntaxKind::INTERFACE_DECL;
        let is_object = node.kind() == SyntaxKind::OBJECT_DECL;
        let is_companion = is_object && written.has(SyntaxKind::COMPANION_KW);
        let is_enum = node.kind() == SyntaxKind::CLASS_DECL && written.has(SyntaxKind::ENUM_KW);
        let type_params: Vec<String> = SymbolExtractor::type_parameters_of(node)
            .into_iter()
            .map(|param| param.name.to_string())
            .collect();

        let owner = if is_companion {
            Owner::Companion
        } else if is_object {
            Owner::Object
        } else {
            Owner::Class
        };

        // `Owner.Companion` is reachable through a static field of the owner.
        if is_companion {
            if let Some(outer) = outer_of_companion {
                let simple = fqn.rsplit('.').next().unwrap_or("Companion");
                self.field(
                    outer,
                    simple,
                    Some(fqn.to_string()),
                    TextRange::new(
                        node.text_range().start().into(),
                        node.text_range().start().into(),
                    ),
                    true,
                    fqn,
                );
            }
        }

        if is_enum {
            self.enum_statics(node, fqn);
        }

        // Primary-constructor properties.
        for param in constructor_params(node) {
            let Some(is_var) = param.property else {
                continue;
            };
            let annotations = &param.written;
            self.accessors(
                fqn,
                owner,
                &param.name,
                param.name_range,
                Some(param.kotlin_type.clone()),
                is_var,
                annotations,
                PropertyFlags::default(),
                &type_params,
                None,
                outer_of_companion,
            );
        }

        let Some(body) = node
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_BODY)
        else {
            return;
        };

        for member in body.children() {
            match member.kind() {
                SyntaxKind::FUNCTION_DECL => {
                    self.function_in(
                        &member,
                        fqn,
                        owner,
                        &type_params,
                        outer_of_companion,
                        is_interface,
                    );
                }
                SyntaxKind::PROPERTY_DECL => {
                    self.property_in(&member, fqn, owner, &type_params, outer_of_companion);
                }
                SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL | SyntaxKind::OBJECT_DECL => {
                    if let Some(nested) = self.declared_fqn(&member, Some(fqn)) {
                        let companion_owner = if member.kind() == SyntaxKind::OBJECT_DECL
                            && written_before(&member).has(SyntaxKind::COMPANION_KW)
                        {
                            Some(fqn)
                        } else {
                            None
                        };
                        self.class_like(&member, &nested, companion_owner);
                    }
                }
                _ => {}
            }
        }
    }

    fn enum_statics(&mut self, node: &SyntaxNode, fqn: &str) {
        let range = node.text_range();
        let range = TextRange::new(range.start().into(), range.start().into());
        self.push_method(MethodSpec {
            container: fqn,
            name: "values",
            range,
            is_static: true,
            params: Vec::new(),
            return_type: Some(format!("{fqn}[]")),
            origin: fqn,
            type_parameters: Vec::new(),
            protected: false,
        });
        self.push_method(MethodSpec {
            container: fqn,
            name: "valueOf",
            range,
            is_static: true,
            params: vec![("java.lang.String".to_string(), "name".to_string())],
            return_type: Some(fqn.to_string()),
            origin: fqn,
            type_parameters: Vec::new(),
            protected: false,
        });
    }

    // ---------------------------------------------------------------- functions

    /// A top-level function. Returns whether a facade member was produced.
    fn function(&mut self, node: &SyntaxNode, facade: &str, owner: Owner) -> bool {
        self.function_in(node, facade, owner, &[], None, false)
    }

    fn function_in(
        &mut self,
        node: &SyntaxNode,
        container: &str,
        owner: Owner,
        class_type_params: &[String],
        outer_of_companion: Option<&str>,
        _in_interface: bool,
    ) -> bool {
        let written = written_before(node);
        let is_jvm_static = written.has_annotation("JvmStatic");
        // Functions of a class or `object` already exist as Kotlin `Function`
        // symbols, and a Java-shaped duplicate under the same `Owner.name`
        // prefix would make the by-FQN lookup treat the name as an overload
        // family. The view therefore adds only what lives under a different
        // owner than its Kotlin twin: file-facade members and the static
        // forwarders a companion's `@JvmStatic` function gets on its class.
        let needs_view = owner == Owner::Facade || (owner == Owner::Companion && is_jvm_static);
        if !needs_view {
            return false;
        }
        if written.has(SyntaxKind::PRIVATE_KW)
            || (owner != Owner::Facade && written.has(SyntaxKind::INTERNAL_KW))
            || written.has(SyntaxKind::SUSPEND_KW)
        {
            return false;
        }

        let Some(header) = function_header(node) else {
            return false;
        };
        // `inline fun <reified T>` has no JVM signature Java could call.
        if written.has(SyntaxKind::INLINE_KW)
            && node
                .children()
                .find(|child| child.kind() == SyntaxKind::TYPE_PARAMETER_LIST)
                .is_some_and(|list| {
                    list.descendants_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .any(|token| token.kind() == SyntaxKind::REIFIED_KW)
                })
        {
            return false;
        }
        let mut type_params: Vec<String> = class_type_params.to_vec();
        let own_type_params = SymbolExtractor::type_parameters_of(node);
        type_params.extend(own_type_params.iter().map(|param| param.name.to_string()));

        let jvm_name = written
            .annotation("JvmName")
            .and_then(|annotation| annotation.first_string.clone())
            .unwrap_or_else(|| header.name.clone());

        let mut params: Vec<Param> = header.params.clone();
        // Extension: receiver first.
        let receiver = header.receiver.clone();

        // Where the JVM members live: (owner class, static?).
        let mut targets: Vec<(String, bool)> = Vec::new();
        match owner {
            Owner::Facade => targets.push((container.to_string(), true)),
            Owner::Class => targets.push((container.to_string(), false)),
            Owner::Object => targets.push((container.to_string(), is_jvm_static)),
            Owner::Companion => {
                // A static forwarder on the owner for `@JvmStatic`; the
                // companion's own instance method is the Kotlin function.
                if is_jvm_static {
                    if let Some(outer) = outer_of_companion {
                        targets.push((outer.to_string(), true));
                    }
                }
            }
        }
        if targets.is_empty() {
            return false;
        }

        let origin = self.origin_fqn(container, owner, &header.name);
        let return_type = header
            .return_type
            .as_ref()
            .map(|kotlin| self.jvm_type(kotlin, &type_params, false));

        let mut jvm_params: Vec<(String, String)> = Vec::new();
        if let Some(receiver) = &receiver {
            jvm_params.push((
                self.jvm_type(receiver, &type_params, false),
                "receiver".to_string(),
            ));
        }
        let default_count = params.iter().filter(|param| param.has_default).count();
        let overloads = written.has_annotation("JvmOverloads");
        for param in &mut params {
            let ty = if param.is_vararg {
                let element = self.jvm_type(&param.kotlin_type, &type_params, false);
                format!("{element}...")
            } else {
                self.jvm_type(&param.kotlin_type, &type_params, false)
            };
            jvm_params.push((ty, param.name.clone()));
        }

        // Full signature first, then each `@JvmOverloads` truncation.
        let mut signatures = vec![jvm_params.clone()];
        if overloads && default_count > 0 {
            let leading = usize::from(receiver.is_some());
            let mut kept: Vec<bool> = params.iter().map(|_| true).collect();
            for index in (0..params.len()).rev() {
                if params[index].has_default {
                    kept[index] = false;
                    let mut signature: Vec<(String, String)> = Vec::new();
                    if leading == 1 {
                        signature.push(jvm_params[0].clone());
                    }
                    for (param_index, keep) in kept.iter().enumerate() {
                        if *keep {
                            signature.push(jvm_params[leading + param_index].clone());
                        }
                    }
                    signatures.push(signature);
                }
            }
        }

        let range = header.name_range;
        for (target, is_static) in &targets {
            for signature in &signatures {
                self.push_method(MethodSpec {
                    container: target,
                    name: &jvm_name,
                    range,
                    is_static: *is_static,
                    params: signature.clone(),
                    return_type: return_type.clone(),
                    origin: &origin,
                    type_parameters: own_type_params.clone(),
                    protected: written.protected(),
                });
            }
        }
        true
    }

    // --------------------------------------------------------------- properties

    fn property(&mut self, node: &SyntaxNode, container: &str, owner: Owner) -> bool {
        self.property_in(node, container, owner, &[], None)
    }

    fn property_in(
        &mut self,
        node: &SyntaxNode,
        container: &str,
        owner: Owner,
        class_type_params: &[String],
        outer_of_companion: Option<&str>,
    ) -> bool {
        let written = written_before(node);
        let Some(info) = property_header(node) else {
            return false;
        };
        let flags = PropertyFlags {
            is_const: written.is_const(node),
            has_custom_getter: info.getter.is_some(),
            getter_hidden: info
                .getter
                .as_ref()
                .is_some_and(|accessor| accessor.has(SyntaxKind::PRIVATE_KW)),
            setter_hidden: info.setter.as_ref().is_some_and(|accessor| {
                accessor.has(SyntaxKind::PRIVATE_KW) || accessor.has(SyntaxKind::PROTECTED_KW)
            }),
        };
        self.accessors(
            container,
            owner,
            &info.name,
            info.name_range,
            info.kotlin_type.clone(),
            info.is_var,
            &written,
            flags,
            class_type_params,
            info.receiver.clone(),
            outer_of_companion,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn accessors(
        &mut self,
        container: &str,
        owner: Owner,
        name: &str,
        name_range: TextRange,
        kotlin_type: Option<String>,
        is_var: bool,
        written: &Written,
        flags: PropertyFlags,
        type_params: &[String],
        receiver: Option<String>,
        outer_of_companion: Option<&str>,
    ) -> bool {
        if written.hidden_from_java() && !flags.has_custom_getter {
            return false;
        }
        if written.has(SyntaxKind::PRIVATE_KW) || written.has(SyntaxKind::INTERNAL_KW) {
            return false;
        }
        let ty = kotlin_type
            .as_ref()
            .map(|kotlin| self.jvm_type(kotlin, type_params, false));
        let origin = self.origin_fqn(container, owner, name);

        // Fields: `@JvmField`, `const`, and members of objects that are static
        // on the owner.
        let is_field = written.has_annotation("JvmField") || flags.is_const;
        if is_field {
            let (target, is_static) = match owner {
                Owner::Facade => (container.to_string(), true),
                Owner::Companion => match outer_of_companion {
                    Some(outer) => (outer.to_string(), true),
                    None => return false,
                },
                Owner::Object => (container.to_string(), true),
                Owner::Class => (container.to_string(), false),
            };
            // Instance `@JvmField` fields are ordinary fields of the class;
            // the Kotlin `Property` symbol already stands for them.
            if owner == Owner::Class {
                return false;
            }
            self.field(&target, name, ty, name_range, is_static, &origin);
            return true;
        }

        let jvm_static = matches!(owner, Owner::Facade) || written.has_annotation("JvmStatic");
        let (target, is_static) = match owner {
            Owner::Facade => (container.to_string(), true),
            Owner::Companion => {
                if !written.has_annotation("JvmStatic") {
                    return false; // reached through `Owner.Companion` only
                }
                match outer_of_companion {
                    Some(outer) => (outer.to_string(), true),
                    None => return false,
                }
            }
            Owner::Object | Owner::Class => (container.to_string(), jvm_static),
        };

        let (getter, setter) = accessor_names(name);
        let getter_produced = !flags.getter_hidden;
        if getter_produced {
            let mut params: Vec<(String, String)> = Vec::new();
            if let Some(receiver) = &receiver {
                params.push((
                    self.jvm_type(receiver, type_params, false),
                    "receiver".to_string(),
                ));
            }
            self.push_method(MethodSpec {
                container: &target,
                name: &getter,
                range: name_range,
                is_static,
                params,
                return_type: ty.clone(),
                origin: &origin,
                type_parameters: Vec::new(),
                protected: written.protected(),
            });
        }
        let setter_produced = is_var && !flags.setter_hidden;
        if setter_produced {
            let mut params: Vec<(String, String)> = Vec::new();
            if let Some(receiver) = &receiver {
                params.push((
                    self.jvm_type(receiver, type_params, false),
                    "receiver".to_string(),
                ));
            }
            params.push((
                ty.clone().unwrap_or_else(|| "java.lang.Object".to_string()),
                "value".to_string(),
            ));
            self.push_method(MethodSpec {
                container: &target,
                name: &setter,
                range: name_range,
                is_static,
                params,
                return_type: Some("void".to_string()),
                origin: &origin,
                type_parameters: Vec::new(),
                protected: written.protected(),
            });
        }
        getter_produced || setter_produced
    }

    // ------------------------------------------------------------------ output

    fn field(
        &mut self,
        container: &str,
        name: &str,
        jvm_type: Option<String>,
        range: TextRange,
        is_static: bool,
        origin: &str,
    ) {
        self.out.push(Symbol {
            fqn: format!("{container}.{name}"),
            simple_name: name.to_string(),
            range,
            kind: SymbolKind::FIELD,
            modifiers: Modifiers {
                is_public: true,
                is_static,
                is_final: true,
                ..Modifiers::default()
            },
            container: Some(container.to_string()),
            return_type: jvm_type,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            custom_getter: false,
            custom_setter: false,
            getter_visibility: None,
            setter_visibility: None,
            type_parameters: Vec::new(),
            constructor_params: Vec::new(),
            is_synthetic: true,
            receiver_type: None,
            jvm_origin: Some(origin.to_string()),
        });
    }

    fn push_method(&mut self, spec: MethodSpec<'_>) {
        let descriptor = spec
            .params
            .iter()
            .map(|(ty, _)| erase(ty))
            .collect::<Vec<_>>()
            .join(",");
        self.out.push(Symbol {
            fqn: format!("{}.{}({descriptor})", spec.container, spec.name),
            simple_name: spec.name.to_string(),
            range: spec.range,
            kind: SymbolKind::METHOD,
            modifiers: Modifiers {
                is_public: !spec.protected,
                is_protected: spec.protected,
                is_static: spec.is_static,
                ..Modifiers::default()
            },
            container: Some(spec.container.to_string()),
            return_type: spec.return_type,
            parameters: spec
                .params
                .iter()
                .map(|(ty, name)| format!("{ty} {name}"))
                .collect(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            custom_getter: false,
            custom_setter: false,
            getter_visibility: None,
            setter_visibility: None,
            type_parameters: spec.type_parameters,
            constructor_params: Vec::new(),
            is_synthetic: true,
            receiver_type: None,
            jvm_origin: Some(spec.origin.to_string()),
        });
    }

    // ------------------------------------------------------------ type mapping

    /// Java spelling of a Kotlin type written in this file.
    ///
    /// `boxed` forces reference types (type arguments cannot be primitives).
    fn jvm_type(&self, kotlin: &str, type_params: &[String], boxed: bool) -> String {
        let text: String = kotlin.chars().filter(|c| !c.is_whitespace()).collect();
        let (text, nullable) = match text.strip_suffix('?') {
            Some(stripped) => (stripped.to_string(), true),
            None => (text, false),
        };
        let boxed = boxed || nullable;

        // Function types
        if text.contains("->") {
            let arity = function_type_arity(&text);
            return format!("kotlin.jvm.functions.Function{arity}");
        }

        // Generic application `Base<A,B>`
        if let Some(open) = text.find('<') {
            if text.ends_with('>') {
                let base = &text[..open];
                let arguments = split_top_level(&text[open + 1..text.len() - 1]);
                let base = self.jvm_type(base, type_params, true);
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|argument| {
                        let argument = argument
                            .trim_start_matches("out")
                            .trim_start_matches("in")
                            .to_string();
                        if argument == "*" {
                            "?".to_string()
                        } else {
                            self.jvm_type(&argument, type_params, true)
                        }
                    })
                    .collect();
                if base == "java.lang.Object[]" || text.starts_with("Array<") {
                    return format!("{}[]", arguments.first().cloned().unwrap_or_default());
                }
                return format!("{base}<{}>", arguments.join(","));
            }
        }

        if type_params.contains(&text) {
            return text;
        }

        let mapped = match text.as_str() {
            "Int" => Some(("int", "java.lang.Integer")),
            "Long" => Some(("long", "java.lang.Long")),
            "Short" => Some(("short", "java.lang.Short")),
            "Byte" => Some(("byte", "java.lang.Byte")),
            "Char" => Some(("char", "java.lang.Character")),
            "Boolean" => Some(("boolean", "java.lang.Boolean")),
            "Float" => Some(("float", "java.lang.Float")),
            "Double" => Some(("double", "java.lang.Double")),
            "Unit" => Some(("void", "kotlin.Unit")),
            _ => None,
        };
        if let Some((primitive, reference)) = mapped {
            return if boxed { reference } else { primitive }.to_string();
        }

        let array = match text.as_str() {
            "IntArray" => Some("int[]"),
            "LongArray" => Some("long[]"),
            "ShortArray" => Some("short[]"),
            "ByteArray" => Some("byte[]"),
            "CharArray" => Some("char[]"),
            "BooleanArray" => Some("boolean[]"),
            "FloatArray" => Some("float[]"),
            "DoubleArray" => Some("double[]"),
            _ => None,
        };
        if let Some(array) = array {
            return array.to_string();
        }

        let known = match text.as_str() {
            "Any" => Some("java.lang.Object"),
            "Nothing" => Some("java.lang.Void"),
            "String" => Some("java.lang.String"),
            "CharSequence" => Some("java.lang.CharSequence"),
            "Number" => Some("java.lang.Number"),
            "Throwable" => Some("java.lang.Throwable"),
            "Exception" => Some("java.lang.Exception"),
            "RuntimeException" => Some("java.lang.RuntimeException"),
            "IllegalArgumentException" => Some("java.lang.IllegalArgumentException"),
            "IllegalStateException" => Some("java.lang.IllegalStateException"),
            "Error" => Some("java.lang.Error"),
            "Comparable" => Some("java.lang.Comparable"),
            "Enum" => Some("java.lang.Enum"),
            "Runnable" => Some("java.lang.Runnable"),
            "StringBuilder" => Some("java.lang.StringBuilder"),
            "Class" => Some("java.lang.Class"),
            "List" | "MutableList" => Some("java.util.List"),
            "Collection" | "MutableCollection" => Some("java.util.Collection"),
            "Iterable" | "MutableIterable" => Some("java.lang.Iterable"),
            "Iterator" | "MutableIterator" => Some("java.util.Iterator"),
            "Set" | "MutableSet" => Some("java.util.Set"),
            "Map" | "MutableMap" => Some("java.util.Map"),
            "Map.Entry" | "MutableMap.MutableEntry" => Some("java.util.Map.Entry"),
            "Comparator" => Some("java.util.Comparator"),
            "Pair" => Some("kotlin.Pair"),
            "Triple" => Some("kotlin.Triple"),
            "Lazy" => Some("kotlin.Lazy"),
            "Result" => Some("kotlin.Result"),
            "Regex" => Some("kotlin.text.Regex"),
            "Sequence" => Some("kotlin.sequences.Sequence"),
            "IntRange" => Some("kotlin.ranges.IntRange"),
            "KClass" => Some("kotlin.reflect.KClass"),
            _ => None,
        };
        if let Some(known) = known {
            return known.to_string();
        }

        // Qualified name as written (`java.math.BigDecimal`, `Outer.Inner`).
        let first = text.split('.').next().unwrap_or(&text);
        if let Some(imported) = self.imports.explicit.get(first) {
            let rest = &text[first.len()..];
            return format!("{imported}{rest}");
        }
        if first.chars().next().is_some_and(char::is_lowercase) && text.contains('.') {
            return text;
        }
        // Wildcard imports resolve by index at use; an unqualified simple name
        // that is neither imported nor known is taken from this package.
        self.qualify_top(&text)
    }
}

struct MethodSpec<'a> {
    container: &'a str,
    name: &'a str,
    range: TextRange,
    is_static: bool,
    params: Vec<(String, String)>,
    return_type: Option<String>,
    origin: &'a str,
    type_parameters: Vec<TypeParameterInfo>,
    protected: bool,
}

#[derive(Default, Clone, Copy)]
struct PropertyFlags {
    is_const: bool,
    has_custom_getter: bool,
    getter_hidden: bool,
    setter_hidden: bool,
}

use hyperion_types::jvm_names::accessor_names;

/// Erasure of a Java type spelling for a descriptor: generics dropped.
fn erase(ty: &str) -> String {
    let Some(open) = ty.find('<') else {
        return ty.to_string();
    };
    let suffix = if ty.ends_with("...") {
        "..."
    } else if ty.ends_with("[]") {
        "[]"
    } else {
        ""
    };
    format!("{}{suffix}", &ty[..open])
}

/// Split `A,Map<B,C>,D` at top-level commas.
fn split_top_level(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '<' | '(' => {
                depth += 1;
                current.push(c);
            }
            '>' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Number of parameters of a Kotlin function type `(A,B)->C` / `R.(A)->C`.
fn function_type_arity(text: &str) -> usize {
    let Some(open) = text.find('(') else {
        return 0;
    };
    let receiver = usize::from(open > 0 && text[..open].ends_with('.'));
    let mut depth = 0i32;
    let mut close = None;
    for (index, c) in text[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + index);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return receiver;
    };
    let inner = &text[open + 1..close];
    let count = if inner.is_empty() {
        0
    } else {
        split_top_level(inner).len()
    };
    count + receiver
}

// ------------------------------------------------------------------- CST facts

struct FunctionHeader {
    name: String,
    name_range: TextRange,
    receiver: Option<String>,
    return_type: Option<String>,
    params: Vec<Param>,
}

fn node_text_compact(node: &SyntaxNode) -> String {
    node.descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia())
        .map(|token| token.text().to_string())
        .collect::<String>()
}

fn type_node_text(node: &SyntaxNode) -> String {
    // Annotations inside a type (`@Composable () -> Unit`) are not part of it.
    node.children_with_tokens()
        .filter(|element| {
            !matches!(element, NodeOrToken::Node(child) if child.kind() == SyntaxKind::ANNOTATION)
        })
        .map(|element| match element {
            NodeOrToken::Token(token) => {
                if token.kind().is_trivia() {
                    String::new()
                } else {
                    token.text().to_string()
                }
            }
            NodeOrToken::Node(child) => node_text_compact(&child),
        })
        .collect::<String>()
}

const fn is_type_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::TYPE_REF | SyntaxKind::FUNCTION_TYPE | SyntaxKind::NULLABLE_TYPE
    )
}

fn function_header(node: &SyntaxNode) -> Option<FunctionHeader> {
    let mut receiver = None;
    let mut return_type = None;
    let mut name: Option<(String, TextRange)> = None;
    let mut seen_colon = false;
    let mut params = Vec::new();

    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) => match child.kind() {
                SyntaxKind::PARAMETER_LIST => params = parameters(&child),
                kind if is_type_node(kind) => {
                    if name.is_none() && !seen_colon {
                        receiver = Some(type_node_text(&child));
                    } else if seen_colon && return_type.is_none() {
                        return_type = Some(type_node_text(&child));
                    }
                }
                _ => {}
            },
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::COLON => seen_colon = true,
                SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT if name.is_none() => {
                    name = Some((
                        token.text().trim_matches('`').to_string(),
                        TextRange::new(
                            token.text_range().start().into(),
                            token.text_range().end().into(),
                        ),
                    ));
                }
                _ => {}
            },
        }
    }

    let (name, name_range) = name?;
    Some(FunctionHeader {
        name,
        name_range,
        receiver,
        return_type,
        params,
    })
}

fn parameters(list: &SyntaxNode) -> Vec<Param> {
    list.children()
        .filter(|child| child.kind() == SyntaxKind::PARAMETER)
        .filter_map(|parameter| {
            let mut name: Option<(String, TextRange)> = None;
            let mut kotlin_type = String::new();
            let mut has_default = false;
            let mut is_vararg = false;
            let mut property = None;
            let mut written = Written::default();
            let mut seen_colon = false;
            for element in parameter.children_with_tokens() {
                match element {
                    NodeOrToken::Token(token) => match token.kind() {
                        SyntaxKind::VAL_KW => property = Some(false),
                        SyntaxKind::VAR_KW => property = Some(true),
                        SyntaxKind::VARARG_KW => is_vararg = true,
                        SyntaxKind::COLON => seen_colon = true,
                        SyntaxKind::EQ => has_default = true,
                        SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT if !seen_colon => {
                            name = Some((
                                token.text().trim_matches('`').to_string(),
                                TextRange::new(
                                    token.text_range().start().into(),
                                    token.text_range().end().into(),
                                ),
                            ));
                        }
                        kind if !seen_colon && !kind.is_trivia() => written.keywords.push(kind),
                        _ => {}
                    },
                    NodeOrToken::Node(child) => {
                        if child.kind() == SyntaxKind::ANNOTATION {
                            written.annotations.push(read_annotation(&child));
                        } else if is_type_node(child.kind()) && kotlin_type.is_empty() {
                            kotlin_type = type_node_text(&child);
                        }
                    }
                }
            }
            let (name, name_range) = name?;
            Some(Param {
                name,
                kotlin_type,
                has_default,
                is_vararg,
                property,
                written,
                name_range,
            })
        })
        .collect()
}

fn constructor_params(class_node: &SyntaxNode) -> Vec<Param> {
    class_node
        .children()
        .find(|child| child.kind() == SyntaxKind::PARAMETER_LIST)
        .map(|list| parameters(&list))
        .unwrap_or_default()
}

struct PropertyHeader {
    name: String,
    name_range: TextRange,
    kotlin_type: Option<String>,
    is_var: bool,
    receiver: Option<String>,
    getter: Option<Written>,
    setter: Option<Written>,
}

fn property_header(node: &SyntaxNode) -> Option<PropertyHeader> {
    let mut is_var = false;
    let mut receiver = None;
    let mut kotlin_type = None;
    let mut name: Option<(String, TextRange)> = None;
    let mut seen_colon = false;
    let mut getter = None;
    let mut setter = None;

    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::VAR_KW => is_var = true,
                SyntaxKind::COLON => seen_colon = true,
                SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT if name.is_none() && !seen_colon => {
                    name = Some((
                        token.text().trim_matches('`').to_string(),
                        TextRange::new(
                            token.text_range().start().into(),
                            token.text_range().end().into(),
                        ),
                    ));
                }
                _ => {}
            },
            NodeOrToken::Node(child) => match child.kind() {
                kind if is_type_node(kind) => {
                    if name.is_none() && !seen_colon {
                        receiver = Some(type_node_text(&child));
                    } else if seen_colon && kotlin_type.is_none() {
                        kotlin_type = Some(type_node_text(&child));
                    }
                }
                SyntaxKind::PROPERTY_ACCESSOR => {
                    let mut written = Written::default();
                    for inner in child.children() {
                        if inner.kind() == SyntaxKind::MODIFIERS {
                            collect_written(&inner, &mut written);
                        }
                    }
                    let is_setter = child
                        .children_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .any(|token| token.kind() == SyntaxKind::IDENT && token.text() == "set");
                    if is_setter {
                        setter = Some(written);
                    } else {
                        getter = Some(written);
                    }
                }
                _ => {}
            },
        }
    }

    let (name, name_range) = name?;
    Some(PropertyHeader {
        name,
        name_range,
        kotlin_type,
        is_var,
        receiver,
        getter,
        setter,
    })
}

fn class_like_name(node: &SyntaxNode) -> Option<String> {
    let mut seen_keyword = false;
    for token in node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
    {
        if matches!(
            token.kind(),
            SyntaxKind::CLASS_KW | SyntaxKind::OBJECT_KW | SyntaxKind::INTERFACE_KW
        ) {
            seen_keyword = true;
        } else if seen_keyword
            && matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT)
        {
            return Some(token.text().trim_matches('`').to_string());
        }
    }
    if node.kind() == SyntaxKind::OBJECT_DECL && written_before(node).has(SyntaxKind::COMPANION_KW)
    {
        return Some("Companion".to_string());
    }
    None
}
