// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! How debuggee values are shown to the client: runtime type, a readable
//! summary (strings, boxed primitives, collections with their size, maps,
//! `toString()` for other objects) and their children when expanded.

use std::time::Duration;

use hyperion_jdwp::commands::Command;
use hyperion_jdwp::replies::{self, tag, FieldInfo, JdwpValue};

use super::evaluate::{unbox_signature, Prim};
use super::presentation::{format_jdwp_value, jni_sig_to_display};
use super::state::{ObjectKind, ObjectRefInfo};
use super::DebugSession;
use crate::types::Variable;

/// Longest a `toString()` shown in the variables view may run.
const TO_STRING_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TEXT_CHARS: usize = 200;
const MAX_CHILDREN: usize = 100;
/// Collections larger than this are summarized without materializing elements.
const MAX_COLLECTION_EXPANSION: i32 = 10_000;
const ACC_STATIC: i32 = 0x0008;

/// A value ready for a DAP `variables`/`evaluate` response.
#[derive(Debug, Clone)]
pub(super) struct Presented {
    pub text: String,
    pub type_name: Option<String>,
    pub variables_reference: u64,
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(limit).collect();
    short.push('…');
    short
}

const fn primitive_type_name(value: &JdwpValue) -> Option<&'static str> {
    Some(match value {
        JdwpValue::Boolean(_) => "boolean",
        JdwpValue::Byte(_) => "byte",
        JdwpValue::Char(_) => "char",
        JdwpValue::Short(_) => "short",
        JdwpValue::Int(_) => "int",
        JdwpValue::Long(_) => "long",
        JdwpValue::Float(_) => "float",
        JdwpValue::Double(_) => "double",
        _ => return None,
    })
}

impl DebugSession {
    /// Present any value; objects get a runtime type and, when they have
    /// something to show, a `variablesReference` to expand.
    pub(super) async fn present(&mut self, value: &JdwpValue, thread_id: u64) -> Presented {
        match value {
            JdwpValue::Object { tag, object_id } if *object_id != 0 => {
                self.present_object(*tag, *object_id, thread_id).await
            }
            JdwpValue::Null | JdwpValue::Object { .. } => Presented {
                text: "null".to_owned(),
                type_name: None,
                variables_reference: 0,
            },
            other => Presented {
                text: format_jdwp_value(other),
                type_name: primitive_type_name(other).map(str::to_owned),
                variables_reference: 0,
            },
        }
    }

    async fn present_object(
        &mut self,
        object_tag: u8,
        object_id: u64,
        thread_id: u64,
    ) -> Presented {
        let Some(type_id) = self.runtime_type(object_id).await else {
            return Presented {
                text: format!("Object@{object_id:x}"),
                type_name: None,
                variables_reference: 0,
            };
        };
        let signature = self
            .get_class_signature(type_id)
            .await
            .unwrap_or_else(|| "Ljava/lang/Object;".to_owned());
        let type_name = jni_sig_to_display(&signature);
        let mut presented = Presented {
            text: String::new(),
            type_name: Some(type_name.clone()),
            variables_reference: 0,
        };

        // Strings: the contents, quoted.
        if object_tag == tag::STRING || signature == "Ljava/lang/String;" {
            let text = self
                .resolve_string_value(object_id)
                .await
                .unwrap_or_default();
            presented.text = format!("\"{}\"", truncate(&text, MAX_TEXT_CHARS * 5));
            return presented;
        }

        // Arrays: `int[3]`, expandable.
        if object_tag == tag::ARRAY || signature.starts_with('[') {
            let length = self.array_length(object_id).await.unwrap_or(0);
            presented.text = format!("{}[{length}]", jni_sig_to_display(&signature[1..]));
            presented.variables_reference =
                self.alloc_object_ref(object_id, thread_id, ObjectKind::Array);
            return presented;
        }

        // Boxed primitives: the value itself.
        if unbox_signature(&signature).is_some() {
            if let Some(prim) = self.unboxed(object_id, type_id).await {
                presented.text = prim.display();
                return presented;
            }
        }

        // Threads, class loaders and friends: never run their toString().
        let skip_invoke = matches!(
            object_tag,
            tag::THREAD | tag::THREAD_GROUP | tag::CLASS_LOADER
        );

        let is_map = self.is_subtype(type_id, "Ljava/util/Map;").await;
        let is_collection = !is_map && self.is_subtype(type_id, "Ljava/util/Collection;").await;
        if (is_map || is_collection) && !skip_invoke {
            let size = self.call_int_method(object_id, "size", thread_id).await;
            let mut text = size.map_or_else(String::new, |n| format!("size={n}"));
            let preview_fits = size.is_some_and(|n| n <= 20);
            if preview_fits || size.is_none() {
                if let Some(preview) = self.call_to_string(object_id, thread_id).await {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&truncate(&preview, 120));
                }
            }
            if text.is_empty() {
                text = self.default_object_text(object_id).await;
            }
            presented.text = text;
            presented.variables_reference = self.alloc_object_ref(
                object_id,
                thread_id,
                if is_map {
                    ObjectKind::Map
                } else {
                    ObjectKind::Collection
                },
            );
            return presented;
        }

        // Anything else: `toString()` when the class defines one.
        let text = if skip_invoke {
            None
        } else {
            self.call_to_string_if_overridden(object_id, type_id, thread_id)
                .await
        };
        presented.text = match text {
            Some(text) => truncate(&text, MAX_TEXT_CHARS),
            None => self.default_object_text(object_id).await,
        };
        presented.variables_reference =
            self.alloc_object_ref(object_id, thread_id, ObjectKind::Plain);
        presented
    }

    fn alloc_object_ref(&mut self, object_id: u64, thread_id: u64, kind: ObjectKind) -> u64 {
        let reference = self.alloc_var_ref();
        self.variables.objects.insert(
            reference,
            ObjectRefInfo {
                object_id,
                thread_id,
                kind,
            },
        );
        reference
    }

    /// `Type@1a2b`, the fallback text of an object.
    pub(super) async fn default_object_text(&mut self, object_id: u64) -> String {
        let name = match self.runtime_type(object_id).await {
            Some(type_id) => self
                .get_class_signature(type_id)
                .await
                .map(|s| jni_sig_to_display(&s)),
            None => None,
        };
        let simple = name
            .as_deref()
            .map_or("Object", |n| n.rsplit('.').next().unwrap_or(n));
        format!("{simple}@{object_id:x}")
    }

    async fn unboxed(&mut self, object_id: u64, type_id: u64) -> Option<Prim> {
        let (declaring, field) = self.lookup_field(type_id, "value").await?;
        let value = self
            .read_field(Some(object_id), declaring, &field)
            .await
            .ok()?;
        Prim::from_jdwp(&value)
    }

    // ---- toString ----

    /// `object.toString()`, run on `thread_id` with a time limit.
    pub(super) async fn call_to_string(
        &mut self,
        object_id: u64,
        thread_id: u64,
    ) -> Option<String> {
        let type_id = self.runtime_type(object_id).await?;
        self.call_to_string_of(object_id, type_id, thread_id, false)
            .await
    }

    async fn call_to_string_if_overridden(
        &mut self,
        object_id: u64,
        type_id: u64,
        thread_id: u64,
    ) -> Option<String> {
        self.call_to_string_of(object_id, type_id, thread_id, true)
            .await
    }

    async fn call_to_string_of(
        &mut self,
        object_id: u64,
        type_id: u64,
        thread_id: u64,
        require_override: bool,
    ) -> Option<String> {
        if thread_id == 0 || self.invoke_disabled {
            return None;
        }
        let (declaring, method) = self
            .find_method(type_id, "toString", &[], false)
            .await
            .ok()?;
        if method.signature != "()Ljava/lang/String;" {
            return None;
        }
        if require_override
            && self.get_class_signature(declaring).await.as_deref() == Some("Ljava/lang/Object;")
        {
            return None;
        }
        let result = self
            .invoke(
                thread_id,
                Some(object_id),
                declaring,
                &method,
                &[],
                TO_STRING_TIMEOUT,
            )
            .await
            .ok()?;
        match result {
            JdwpValue::Object { object_id, .. } if object_id != 0 => {
                self.resolve_string_value(object_id).await
            }
            _ => Some("null".to_owned()),
        }
    }

    /// Call a no-argument `int` method such as `size()`.
    async fn call_int_method(&mut self, object_id: u64, name: &str, thread_id: u64) -> Option<i32> {
        let type_id = self.runtime_type(object_id).await?;
        let (declaring, method) = self.find_method(type_id, name, &[], false).await.ok()?;
        if method.signature != "()I" {
            return None;
        }
        match self
            .invoke(
                thread_id,
                Some(object_id),
                declaring,
                &method,
                &[],
                TO_STRING_TIMEOUT,
            )
            .await
            .ok()?
        {
            JdwpValue::Int(n) => Some(n),
            _ => None,
        }
    }

    /// Call a no-argument method returning an object reference.
    async fn call_object_method(
        &mut self,
        object_id: u64,
        name: &str,
        thread_id: u64,
    ) -> Option<JdwpValue> {
        let type_id = self.runtime_type(object_id).await?;
        let (declaring, method) = self.find_method(type_id, name, &[], false).await.ok()?;
        if !method.signature.starts_with("()") {
            return None;
        }
        self.invoke(
            thread_id,
            Some(object_id),
            declaring,
            &method,
            &[],
            TO_STRING_TIMEOUT,
        )
        .await
        .ok()
    }

    // ---- Expansion ----

    /// The children of an expandable object reference.
    pub(super) async fn expand(&mut self, info: &ObjectRefInfo) -> Result<Vec<Variable>, String> {
        match info.kind {
            ObjectKind::Array => self.expand_array(info).await,
            ObjectKind::Plain => self.expand_fields(info).await,
            ObjectKind::Collection => match self.expand_collection(info).await {
                Some(children) => Ok(children),
                None => self.expand_fields(info).await,
            },
            ObjectKind::Map => match self.expand_map(info).await {
                Some(children) => Ok(children),
                None => self.expand_fields(info).await,
            },
        }
    }

    async fn variable_for(&mut self, name: String, value: &JdwpValue, thread_id: u64) -> Variable {
        let presented = self.present(value, thread_id).await;
        Variable {
            name,
            value: presented.text,
            type_: presented.type_name,
            variables_reference: presented.variables_reference,
        }
    }

    async fn expand_fields(&mut self, info: &ObjectRefInfo) -> Result<Vec<Variable>, String> {
        let type_id = self
            .runtime_type(info.object_id)
            .await
            .ok_or("failed to get the object's type")?;
        // Instance fields of the class and every superclass.
        let mut fields: Vec<FieldInfo> = Vec::new();
        let mut current = Some(type_id);
        for _ in 0..32 {
            let Some(class_id) = current else {
                break;
            };
            fields.extend(
                self.get_fields(class_id)
                    .await?
                    .into_iter()
                    .filter(|f| f.mod_bits & ACC_STATIC == 0),
            );
            current = self.superclass_of(class_id).await;
        }
        if fields.is_empty() {
            return Ok(Vec::new());
        }
        let id_sizes = *self.id_sizes()?;
        let field_ids: Vec<u64> = fields.iter().map(|f| f.field_id).collect();
        let reply = self
            .conn()?
            .send_command(Command::object_get_values(
                info.object_id,
                &field_ids,
                id_sizes.object_id_size,
                id_sizes.field_id_size,
            ))
            .await;
        // A superclass's field ids are not valid on every JVM for a subclass
        // query; fall back to one request per field then.
        let values = match reply {
            Ok(reply) => {
                let mut data = reply.data;
                replies::parse_object_get_values(&mut data, &id_sizes).map_err(|e| e.to_string())?
            }
            Err(_) => {
                let mut values = Vec::new();
                for field in &fields {
                    match self.read_field(Some(info.object_id), type_id, field).await {
                        Ok(value) => values.push(value),
                        Err(_) => values.push(JdwpValue::Null),
                    }
                }
                values
            }
        };
        let mut children = Vec::with_capacity(fields.len());
        for (field, value) in fields.iter().zip(values.iter()) {
            let mut variable = self
                .variable_for(field.name.clone(), value, info.thread_id)
                .await;
            // The declared type is more informative for a null field.
            if matches!(value, JdwpValue::Null) {
                variable.type_ = Some(jni_sig_to_display(&field.signature));
            }
            children.push(variable);
        }
        Ok(children)
    }

    /// Direct superclass (`None` for `Object`, interfaces and arrays).
    async fn superclass_of(&self, type_id: u64) -> Option<u64> {
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::class_type_superclass(
                type_id,
                id_sizes.reference_type_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let id = replies::parse_class_type_superclass(&mut data, &id_sizes).ok()?;
        (id != 0).then_some(id)
    }

    async fn expand_array(&mut self, info: &ObjectRefInfo) -> Result<Vec<Variable>, String> {
        let length = self.array_length(info.object_id).await?;
        let mut children = vec![Variable {
            name: "length".to_owned(),
            value: length.to_string(),
            type_: Some("int".to_owned()),
            variables_reference: 0,
        }];
        if length == 0 {
            return Ok(children);
        }
        let fetch = length.min(MAX_CHILDREN as i32);
        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::array_get_values(
                info.object_id,
                0,
                fetch,
                id_sizes.object_id_size,
            ))
            .await
            .map_err(|e| format!("failed to get array values: {e}"))?;
        let mut data = reply.data;
        let values =
            replies::parse_array_values(&mut data, &id_sizes).map_err(|e| e.to_string())?;
        for (index, value) in values.iter().enumerate() {
            children.push(
                self.variable_for(format!("[{index}]"), value, info.thread_id)
                    .await,
            );
        }
        if length > fetch {
            children.push(Variable {
                name: format!("[{} more...]", length - fetch),
                value: String::new(),
                type_: None,
                variables_reference: 0,
            });
        }
        Ok(children)
    }

    /// Elements of a collection via `toArray()`.
    async fn expand_collection(&mut self, info: &ObjectRefInfo) -> Option<Vec<Variable>> {
        let size = self
            .call_int_method(info.object_id, "size", info.thread_id)
            .await?;
        if size > MAX_COLLECTION_EXPANSION {
            return None;
        }
        let array = self
            .call_object_method(info.object_id, "toArray", info.thread_id)
            .await?;
        let JdwpValue::Object { object_id, .. } = array else {
            return None;
        };
        let elements = self
            .expand_array(&ObjectRefInfo {
                object_id,
                thread_id: info.thread_id,
                kind: ObjectKind::Array,
            })
            .await
            .ok()?;
        // Drop the synthetic `length` entry: the container's size is shown already.
        Some(
            elements
                .into_iter()
                .filter(|v| v.name != "length")
                .collect(),
        )
    }

    /// Entries of a map: `key -> value`.
    async fn expand_map(&mut self, info: &ObjectRefInfo) -> Option<Vec<Variable>> {
        let size = self
            .call_int_method(info.object_id, "size", info.thread_id)
            .await?;
        if size > MAX_COLLECTION_EXPANSION {
            return None;
        }
        let JdwpValue::Object {
            object_id: entry_set,
            ..
        } = self
            .call_object_method(info.object_id, "entrySet", info.thread_id)
            .await?
        else {
            return None;
        };
        let JdwpValue::Object {
            object_id: array, ..
        } = self
            .call_object_method(entry_set, "toArray", info.thread_id)
            .await?
        else {
            return None;
        };
        let length = self.array_length(array).await.ok()?;
        let fetch = length.min(MAX_CHILDREN as i32);
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::array_get_values(
                array,
                0,
                fetch,
                id_sizes.object_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let entries = replies::parse_array_values(&mut data, &id_sizes).ok()?;
        let mut children = Vec::new();
        for entry in entries {
            let JdwpValue::Object {
                object_id: entry_id,
                ..
            } = entry
            else {
                continue;
            };
            let key = self
                .call_object_method(entry_id, "getKey", info.thread_id)
                .await?;
            let value = self
                .call_object_method(entry_id, "getValue", info.thread_id)
                .await?;
            let key_text = self.present(&key, info.thread_id).await.text;
            children.push(self.variable_for(key_text, &value, info.thread_id).await);
        }
        if length > fetch {
            children.push(Variable {
                name: format!("[{} more...]", length - fetch),
                value: String::new(),
                type_: None,
                variables_reference: 0,
            });
        }
        Some(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_truncated_with_an_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 4), "abcd…");
    }
}
