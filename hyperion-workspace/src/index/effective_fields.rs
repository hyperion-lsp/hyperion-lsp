// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Effective receiver-field election under Java inheritance and hiding rules.

use super::effective_methods::{project_effective_field_type_in, ReceiverProjection};
use super::refactoring::type_package_in;
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

type FieldsByName = BTreeMap<String, Vec<ResolvedDefinition>>;

fn definition_order(left: &ResolvedDefinition, right: &ResolvedDefinition) -> std::cmp::Ordering {
    left.target
        .fqn
        .cmp(&right.target.fqn)
        .then(
            left.location
                .file_id
                .as_u32()
                .cmp(&right.location.file_id.as_u32()),
        )
        .then(left.location.range.start.cmp(&right.location.range.start))
}

fn declared_fields_in(
    election: &OwnerElection<'_>,
    owner: &str,
    member_name: Option<&str>,
) -> Vec<ResolvedDefinition> {
    election
        .member_definitions(owner)
        .iter()
        .flat_map(|members| members.iter())
        .filter(|definition| {
            definition.target.namespace == crate::DeclarationNamespace::Value
                && member_name.is_none_or(|name| definition.symbol.simple_name.as_ref() == name)
                && matches!(
                    definition.symbol.kind,
                    SymbolKind::FIELD
                        | SymbolKind::PROPERTY
                        | SymbolKind::CONSTANT
                        | SymbolKind::ENUM_MEMBER
                )
        })
        .cloned()
        .collect()
}

fn field_is_inheritable_in(
    election: &OwnerElection<'_>,
    child: &str,
    field: &crate::Symbol,
) -> bool {
    if field.modifiers.is_private {
        return false;
    }
    let Some(declaring_owner) = field.container.as_deref() else {
        return false;
    };
    if election
        .symbol(declaring_owner)
        .is_some_and(|owner| owner.kind == SymbolKind::INTERFACE)
        || field.modifiers.is_public
        || field.modifiers.is_protected
        || field.modifiers.is_internal
    {
        return true;
    }

    let language = election
        .type_definition(declaring_owner)
        .and_then(|definition| election.inner.file_path(definition.location.file_id))
        .and_then(Language::from_path);
    if !matches!(language, Some(Language::Java) | None) {
        return true;
    }

    type_package_in(election.inner, child) == type_package_in(election.inner, declaring_owner)
}

fn top_level_type_in(election: &OwnerElection<'_>, type_fqn: &str) -> String {
    let mut top_level = type_fqn.to_owned();
    let mut visited = HashSet::new();
    while visited.insert(top_level.clone()) {
        let Some(container) = election
            .symbol(top_level.as_str())
            .and_then(|symbol| symbol.container.clone())
            .filter(|container| {
                election
                    .symbol(container)
                    .is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind))
            })
        else {
            break;
        };
        top_level = container.to_string();
    }

    // Class-file nested-type symbols do not always carry a source-style
    // container. Recover their nest host from indexed nominal prefixes.
    let mut prefix = type_fqn;
    while let Some((candidate, _)) = prefix.rsplit_once('.') {
        if election
            .symbol(candidate)
            .is_some_and(|symbol| crate::is_type_declaration_kind(symbol.kind))
        {
            top_level = candidate.to_owned();
        }
        prefix = candidate;
    }
    top_level
}

fn is_same_or_subtype_in(election: &OwnerElection<'_>, subtype: &str, supertype: &str) -> bool {
    if subtype == supertype {
        return true;
    }
    let mut pending = VecDeque::from([subtype.to_owned()]);
    let mut visited = HashSet::new();
    while let Some(candidate) = pending.pop_front() {
        if !visited.insert(candidate.clone()) {
            continue;
        }
        let (parent, interfaces) = election
            .inner
            .resolved_supertype_roles_with_election(election, &candidate);
        for direct in parent.into_iter().chain(interfaces) {
            if direct == supertype {
                return true;
            }
            pending.push_back(direct);
        }
    }
    false
}

fn field_is_accessible_in(
    election: &OwnerElection<'_>,
    receiver_fqn: &str,
    lexical_access_owners: &[String],
    qualifier: FieldAccessQualifier,
    field: &crate::Symbol,
) -> bool {
    let Some(declaring_owner) = field.container.as_deref() else {
        return false;
    };
    if field.modifiers.is_public
        || election
            .symbol(declaring_owner)
            .is_some_and(|owner| owner.kind == SymbolKind::INTERFACE)
    {
        return true;
    }
    if field.modifiers.is_private {
        let declaring_nest = top_level_type_in(election, declaring_owner);
        return lexical_access_owners
            .iter()
            .any(|owner| top_level_type_in(election, owner) == declaring_nest);
    }

    let declaring_package = type_package_in(election.inner, declaring_owner);
    if lexical_access_owners
        .iter()
        .any(|owner| type_package_in(election.inner, owner) == declaring_package)
    {
        return true;
    }

    if !field.modifiers.is_protected {
        // Java package-private fields and JVM-language `internal` values are
        // not visible to a Java source in another package.
        return false;
    }

    lexical_access_owners.iter().any(|access_owner| {
        is_same_or_subtype_in(election, access_owner, declaring_owner)
            && (qualifier == FieldAccessQualifier::Super
                || is_same_or_subtype_in(election, receiver_fqn, access_owner))
    })
}

fn merge_inherited_field(fields: &mut FieldsByName, name: String, candidate: ResolvedDefinition) {
    let declarations = fields.entry(name).or_default();
    // A diamond may inherit the same declaration through several interface
    // paths. That is one field, not an ambiguity. Distinct declarations with
    // the same name are all retained so callers do not turn an ambiguous Java
    // member into an index-order winner.
    if declarations
        .iter()
        .any(|existing| existing.target == candidate.target)
    {
        return;
    }
    declarations.push(candidate);
    declarations.sort_unstable_by(definition_order);
}

fn effective_receiver_fields_in(
    election: &OwnerElection<'_>,
    receiver: &str,
    member_name: Option<&str>,
    memo: &mut HashMap<String, Arc<FieldsByName>>,
    visiting: &mut HashSet<String>,
) -> Arc<FieldsByName> {
    if let Some(cached) = memo.get(receiver) {
        return Arc::clone(cached);
    }
    if !visiting.insert(receiver.to_owned()) {
        return Arc::new(FieldsByName::new());
    }

    let direct = declared_fields_in(election, receiver, member_name);
    let direct_names = direct
        .iter()
        .map(|definition| definition.symbol.simple_name.to_string())
        .collect::<HashSet<_>>();
    let (parent, mut interfaces) = election
        .inner
        .resolved_supertype_roles_with_election(election, receiver);
    interfaces.sort_unstable();
    interfaces.dedup();

    let mut inherited = FieldsByName::new();
    for supertype in parent.into_iter().chain(interfaces) {
        let supertype_fields =
            effective_receiver_fields_in(election, &supertype, member_name, memo, visiting);
        for (name, candidates) in supertype_fields.iter() {
            // Any declaration in the receiver hides every accessible inherited
            // declaration with the same name, independent of staticness or
            // field type. Inaccessible/private fields were already removed when
            // the direct-supertype member set was built and never reappear.
            if direct_names.contains(name) {
                continue;
            }
            for candidate in candidates {
                if field_is_inheritable_in(election, receiver, &candidate.symbol) {
                    merge_inherited_field(&mut inherited, name.clone(), candidate.clone());
                }
            }
        }
    }

    for definition in direct {
        inherited
            .entry(definition.symbol.simple_name.to_string())
            .or_default()
            .push(definition);
    }
    for definitions in inherited.values_mut() {
        definitions.sort_unstable_by(definition_order);
    }

    visiting.remove(receiver);
    let inherited = Arc::new(inherited);
    memo.insert(receiver.to_owned(), Arc::clone(&inherited));
    inherited
}

fn project_effective_field_in(
    declaration_projection: &mut ReceiverProjection<'_>,
    receiver_projection: &mut ReceiverProjection<'_>,
    definition: &ResolvedDefinition,
) -> ProjectedEffectiveField {
    let declaration_projection_available =
        project_effective_field_type_in(declaration_projection, definition).is_some();
    ProjectedEffectiveField {
        projected_type: declaration_projection_available
            .then(|| project_effective_field_type_in(receiver_projection, definition))
            .flatten(),
        definition: definition.clone(),
    }
}

impl WorkspaceIndex {
    /// Elect and project every effective field under one coherent index read.
    /// Ambiguous names retain each declaration; unavailable types remain
    /// explicit on their individual views.
    pub fn find_projected_effective_fields(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
    ) -> Vec<ProjectedEffectiveField> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let fields = effective_receiver_fields_in(
            &election,
            receiver_fqn,
            None,
            &mut HashMap::new(),
            &mut HashSet::new(),
        );
        let mut declaration_projection =
            ReceiverProjection::new_scoped(&inner, self.source_scope.as_ref(), receiver_fqn);
        let mut receiver_projection = ReceiverProjection::with_arguments_scoped(
            &inner,
            self.source_scope.as_ref(),
            receiver_fqn,
            receiver_arguments,
        );
        fields
            .values()
            .flatten()
            .map(|definition| {
                project_effective_field_in(
                    &mut declaration_projection,
                    &mut receiver_projection,
                    definition,
                )
            })
            .collect()
    }

    /// Resolve and project one unambiguous effective field under one coherent
    /// index read. An indexed declaration with an unavailable projection is
    /// returned explicitly instead of being confused with a missing field.
    pub fn find_projected_effective_field_by_name(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
        member_name: &str,
    ) -> Option<ProjectedEffectiveField> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let fields = effective_receiver_fields_in(
            &election,
            receiver_fqn,
            Some(member_name),
            &mut HashMap::new(),
            &mut HashSet::new(),
        );
        let [definition] = fields.get(member_name)?.as_slice() else {
            return None;
        };
        let mut declaration_projection =
            ReceiverProjection::new_scoped(&inner, self.source_scope.as_ref(), receiver_fqn);
        let mut receiver_projection = ReceiverProjection::with_arguments_scoped(
            &inner,
            self.source_scope.as_ref(),
            receiver_fqn,
            receiver_arguments,
        );
        Some(project_effective_field_in(
            &mut declaration_projection,
            &mut receiver_projection,
            definition,
        ))
    }

    /// Java access check (JLS 6.6) of one already-elected member.
    ///
    /// The same rules as [`Self::resolve_projected_field_access`], usable for
    /// methods and constructors: `public` and interface members are always
    /// accessible; `private` only inside the declaring top-level type;
    /// package-private inside its package; `protected` additionally from
    /// subclasses (through a receiver that is itself a subtype of the
    /// accessing class unless the access is `super.`-qualified).
    /// `lexical_access_owners` are the enclosing types of the access,
    /// nearest first.
    pub fn is_member_accessible(
        &self,
        receiver_fqn: &str,
        lexical_access_owners: &[String],
        qualifier: FieldAccessQualifier,
        member: &crate::Symbol,
    ) -> bool {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        field_is_accessible_in(
            &election,
            receiver_fqn,
            lexical_access_owners,
            qualifier,
            member,
        )
    }

    /// Resolve one field selector with its complete Java access context.
    ///
    /// Member election, ambiguity detection, generic projection, and access
    /// checking all use the same index snapshot. `lexical_access_owners` are
    /// ordered nearest-first and may include enclosing types; this preserves
    /// nestmate-private and nested protected access without guessing from a
    /// package or simple class name at a later feature layer.
    pub fn resolve_projected_field_access(
        &self,
        receiver_fqn: &str,
        receiver_arguments: &[String],
        member_name: &str,
        lexical_access_owners: &[String],
        qualifier: FieldAccessQualifier,
    ) -> ProjectedFieldAccessResolution {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let fields = effective_receiver_fields_in(
            &election,
            receiver_fqn,
            Some(member_name),
            &mut HashMap::new(),
            &mut HashSet::new(),
        );
        let Some(definitions) = fields.get(member_name) else {
            return ProjectedFieldAccessResolution::Missing;
        };
        let [definition] = definitions.as_slice() else {
            return ProjectedFieldAccessResolution::Ambiguous;
        };
        if !field_is_accessible_in(
            &election,
            receiver_fqn,
            lexical_access_owners,
            qualifier,
            &definition.symbol,
        ) {
            return ProjectedFieldAccessResolution::Inaccessible;
        }

        let mut declaration_projection =
            ReceiverProjection::new_scoped(&inner, self.source_scope.as_ref(), receiver_fqn);
        let mut receiver_projection = ReceiverProjection::with_arguments_scoped(
            &inner,
            self.source_scope.as_ref(),
            receiver_fqn,
            receiver_arguments,
        );
        ProjectedFieldAccessResolution::Resolved(project_effective_field_in(
            &mut declaration_projection,
            &mut receiver_projection,
            definition,
        ))
    }

    /// Return every effective value member visible through `receiver_fqn`.
    ///
    /// The election follows canonical superclass/interface edges under one
    /// coherent index read. A declaration on the receiver hides every
    /// inherited field of the same name. Distinct declarations inherited from
    /// separate branches remain in the result because Java treats that name as
    /// ambiguous, while a diamond that reaches the same declaration is
    /// deduplicated. This models receiver membership, not caller access.
    pub fn find_effective_fields(&self, receiver_fqn: &str) -> Vec<Arc<crate::Symbol>> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let fields = effective_receiver_fields_in(
            &election,
            receiver_fqn,
            None,
            &mut HashMap::new(),
            &mut HashSet::new(),
        );
        fields
            .values()
            .flatten()
            .map(|definition| Arc::clone(&definition.symbol))
            .collect()
    }

    /// Resolve one effective value declaration by name.
    ///
    /// The hierarchy election and source-authority selection happen under one
    /// index read. `None` means either that no field is inherited or that the
    /// name is ambiguous; callers must not pick an index-order winner for two
    /// distinct declarations inherited from separate branches.
    pub fn find_effective_field_definition_by_name(
        &self,
        receiver_fqn: &str,
        member_name: &str,
    ) -> Option<ResolvedDefinition> {
        let inner = self.inner.read();
        let election = OwnerElection::new(&inner, self.source_scope.as_ref());
        let fields = effective_receiver_fields_in(
            &election,
            receiver_fqn,
            Some(member_name),
            &mut HashMap::new(),
            &mut HashSet::new(),
        );
        match fields.get(member_name).map(Vec::as_slice) {
            Some([definition]) => Some(definition.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn index_source(index: &WorkspaceIndex, root: &Path, relative: &str, source: &str) {
        index
            .index_file_with_source(&root.join(relative), source)
            .unwrap_or_else(|error| panic!("failed to index {relative}: {error}"));
    }

    fn owners_for(index: &WorkspaceIndex, receiver: &str, name: &str) -> Vec<String> {
        let mut owners = index
            .find_effective_fields(receiver)
            .into_iter()
            .filter(|field| field.simple_name.as_ref() == name)
            .filter_map(|field| field.container.as_deref().map(str::to_owned))
            .collect::<Vec<_>>();
        owners.sort_unstable();
        owners
    }

    #[test]
    fn inherited_field_ambiguity_and_direct_hiding_are_explicit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Flag.java",
            "package p; public interface Flag { int value = 1; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Base.java",
            "package p; public class Base { public int value; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Ambiguous.java",
            "package p; public class Ambiguous extends Base implements Flag {}",
        );
        index_source(
            &index,
            temp.path(),
            "p/Resolved.java",
            "package p; public class Resolved extends Base implements Flag { private int value; }",
        );

        assert_eq!(
            owners_for(&index, "p.Ambiguous", "value"),
            ["p.Base", "p.Flag"]
        );
        assert_eq!(owners_for(&index, "p.Resolved", "value"), ["p.Resolved"]);
    }

    #[test]
    fn a_diamond_inherits_one_field_declaration_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/Root.java",
            "package p; public interface Root { int shared = 1; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/Left.java",
            "package p; public interface Left extends Root {}",
        );
        index_source(
            &index,
            temp.path(),
            "p/Right.java",
            "package p; public interface Right extends Root {}",
        );
        index_source(
            &index,
            temp.path(),
            "p/Diamond.java",
            "package p; public class Diamond implements Left, Right {}",
        );

        assert_eq!(owners_for(&index, "p.Diamond", "shared"), ["p.Root"]);
    }

    #[test]
    fn inaccessible_nearer_fields_block_deeper_fields_from_reappearing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "a/Base.java",
            "package a; public class Base { public int hidden; int packageValue; }",
        );
        index_source(
            &index,
            temp.path(),
            "b/Middle.java",
            "package b; public class Middle extends a.Base { private int hidden; }",
        );
        index_source(
            &index,
            temp.path(),
            "a/SamePackage.java",
            "package a; public class SamePackage extends Base {}",
        );
        index_source(
            &index,
            temp.path(),
            "a/Child.java",
            "package a; public class Child extends b.Middle {}",
        );

        assert!(owners_for(&index, "a.Child", "hidden").is_empty());
        assert!(owners_for(&index, "a.Child", "packageValue").is_empty());
        assert_eq!(
            owners_for(&index, "a.SamePackage", "packageValue"),
            ["a.Base"]
        );
    }

    #[test]
    fn private_hiding_field_blocks_class_and_interface_ancestors_by_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index_source(
            &index,
            temp.path(),
            "p/A.java",
            "package p; public class A { public static int x; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/I.java",
            "package p; public interface I { int x = 1; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/B.java",
            "package p; public class B extends A implements I { private static int x; }",
        );
        index_source(
            &index,
            temp.path(),
            "p/C.java",
            "package p; public class C extends B {}",
        );

        assert!(index
            .find_effective_field_definition_by_name("p.C", "x")
            .is_none());
        assert!(index
            .find_value_definition_with_inheritance("p.C", "x")
            .is_none());
    }
}
