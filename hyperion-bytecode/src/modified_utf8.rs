// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM Modified UTF-8 encoding shared by class-file readers and writers.

use std::borrow::Cow;

pub(crate) fn decode(bytes: &[u8]) -> Result<String, String> {
    cesu8::from_java_cesu8(bytes)
        .map(Cow::into_owned)
        .map_err(|error| error.to_string())
}

pub(crate) fn encode(value: &str) -> Cow<'_, [u8]> {
    cesu8::to_java_cesu8(value)
}
