// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP request handlers extracted from server.rs for maintainability.
//!
//! This module contains handler implementations for various LSP requests,
//! organized by functionality. The main server.rs file implements the
//! LanguageServer trait and delegates to these handlers.

pub mod build_system;
pub mod file_events;
pub mod lifecycle;
pub mod navigation;
pub mod notifications;
