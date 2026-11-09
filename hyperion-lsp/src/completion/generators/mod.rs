// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion generators module
//!
//! Provides specialized generators for different types of completions:
//! - Symbol completions (file-scoped symbols)
//! - Local variable completions (variables, parameters, fields)
//! - Member access completions (methods, fields of an object)
//! - Workspace type completions (classes, interfaces, enums from workspace)
//! - Constructor completions (after 'new' keyword)

pub mod constructor;
pub mod context;
pub mod kotlin_member_access;
pub mod local_variable;
pub mod member_access;
pub mod symbol;
pub mod workspace_type;

pub use constructor::ConstructorCompletionGenerator;
pub use context::{CompletionGenerator, CompletionGeneratorContext};
pub use kotlin_member_access::KotlinMemberAccessCompletionGenerator;
pub use local_variable::LocalVariableCompletionGenerator;
pub use member_access::MemberAccessCompletionGenerator;
pub use symbol::SymbolCompletionGenerator;
pub use workspace_type::WorkspaceTypeCompletionGenerator;
