//! Edits to JSON and JSONC through `jsonc-parser`'s CST, which holds every comment, space and comma
//! of the file: only the nodes an edit touches change, and the rest prints back as it was.

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstArray, CstInputValue, CstNode, CstObject, CstObjectProp, CstRootNode, ObjectPropName};
use serde_json::Value;

use super::refs::Syntax;
use super::{Extras, Obj, Want, Wants, same_text, string};

/// Runs `edit` on the top-level object of `text` and prints the result. An empty `text` starts a
/// new document, which ends with a newline.
pub(super) fn edit(text: &str, edit: impl FnOnce(&CstObject) -> Result<(), String>) -> Result<String, String> {
    let blank = text.trim().is_empty();
    let root = CstRootNode::parse(if blank { "" } else { text }, &ParseOptions::default()).map_err(|e| e.to_string())?;
    let obj = match root.value() {
        None => root.object_value_or_set(),
        Some(value) => value.as_object().ok_or("the top level is not an object")?,
    };
    edit(&obj)?;
    let out = root.to_string();
    Ok(if blank && !out.ends_with('\n') { out + "\n" } else { out })
}

/// The object at `path` under `obj`, if there is one.
pub(super) fn object_at(obj: &CstObject, path: &[String]) -> Result<Option<CstObject>, String> {
    let mut obj = obj.clone();
    for key in path {
        let Some(prop) = obj.get(key) else { return Ok(None) };
        obj = prop.object_value().ok_or_else(|| format!("{key}: not an object"))?;
    }
    Ok(Some(obj))
}

/// Appends `name: value` to the object at `path`, creating the objects on the way that are missing.
pub(super) fn append_at(obj: &CstObject, path: &[String], name: &str, value: CstInputValue) -> Result<(), String> {
    let mut obj = obj.clone();
    for (i, key) in path.iter().enumerate() {
        match obj.get(key) {
            Some(prop) => obj = prop.object_value().ok_or_else(|| format!("{key}: not an object"))?,
            None => {
                let leaf = CstInputValue::Object(vec![(name.into(), value)]);
                let nested = path[i + 1..].iter().rev().fold(leaf, |inner, k| CstInputValue::Object(vec![(k.clone(), inner)]));
                obj.append(key, nested);
                return Ok(());
            }
        }
    }
    obj.append(name, value);
    Ok(())
}

/// Writes the entry `name` in the object at `path`: edited in place when it exists, else appended
/// with `extras`. `wants` sees the entry as it is now, to keep what already reads right.
pub(super) fn upsert_at(obj: &CstObject, path: &[String], name: &str, wants: impl FnOnce(Option<&Obj>) -> Result<Wants, String>, extras: &Extras) -> Result<(), String> {
    let Some(prop) = object_at(obj, path)?.and_then(|servers| servers.get(name)) else {
        return append_at(obj, path, name, fresh(&wants(None)?, extras));
    };
    let entry = prop.object_value().ok_or_else(|| format!("{name}: not an object"))?;
    let Some(Value::Object(now)) = value_of(&entry) else { return Err(format!("{name}: not an object")) };
    sync(&entry, &now, &wants(Some(&now))?);
    Ok(())
}

pub(super) fn remove_at(obj: &CstObject, path: &[String], name: &str) -> Result<(), String> {
    if let Some(prop) = object_at(obj, path)?.and_then(|servers| servers.get(name)) {
        prop.remove();
    }
    Ok(())
}

/// Renames the key in place: only the name's text changes.
pub(super) fn rename_at(obj: &CstObject, path: &[String], from: &str, to: &str) -> Result<(), String> {
    let servers = object_at(obj, path)?;
    if servers.as_ref().is_some_and(|s| s.get(to).is_some()) {
        return Err(format!("{to} already exists"));
    }
    let prop = servers.and_then(|s| s.get(from)).ok_or_else(|| format!("no server {from}"))?;
    match prop.name() {
        Some(ObjectPropName::String(name)) => name.set_raw_value(quoted(to)),
        Some(ObjectPropName::Word(name)) => name.set_raw_value(quoted(to)),
        None => return Err(format!("no server {from}")),
    }
    Ok(())
}

/// Adds `name` to the list `key` of the object at `path` (creating what's missing), or takes every
/// copy of it out. A name already listed isn't added twice.
pub(super) fn set_in_list(obj: &CstObject, path: &[String], key: &str, name: &str, listed: bool) -> Result<(), String> {
    let target = object_at(obj, path)?;
    let list = match target.as_ref().and_then(|t| t.get(key)) {
        Some(prop) => Some(prop.array_value().ok_or_else(|| format!("{key}: not a list"))?),
        None => None,
    };
    let is_name = |node: &CstNode| value_of(node).is_some_and(|v| v.as_str() == Some(name));
    match (listed, target, list) {
        (true, _, Some(list)) if list.elements().iter().any(is_name) => {}
        (true, _, Some(list)) => {
            list.append(name.into());
        }
        (true, Some(target), None) => {
            target.append(key, strings(&[name.to_string()]));
        }
        (true, None, None) => {
            let (last, parents) = path.split_last().ok_or("no place for the list")?;
            append_at(obj, parents, last, CstInputValue::Object(vec![(key.into(), strings(&[name.to_string()]))]))?;
        }
        (false, _, Some(list)) => list.elements().into_iter().filter(is_name).for_each(CstNode::remove),
        (false, _, None) => {}
    }
    Ok(())
}

/// Switches a boolean key to `value` if it's there and differs.
pub(super) fn set_bool(prop: &CstObjectProp, value: bool) {
    match prop.value().and_then(|v| v.as_boolean_lit()) {
        Some(lit) if lit.value() == value => {}
        Some(lit) => lit.set_value(value),
        None => prop.set_value(value.into()),
    }
}

/// Brings an existing entry to `wants`, touching only the keys (and, inside lists and objects,
/// only the items) that read differently.
fn sync(entry: &CstObject, now: &Obj, wants: &Wants) {
    for (key, want) in wants {
        if want.holds(now.get(*key)) {
            continue;
        }
        let prop = entry.get(key);
        match want {
            Want::Keep => {}
            Want::Absent => prop.into_iter().for_each(CstObjectProp::remove),
            Want::List(items) if items.is_empty() => prop.into_iter().for_each(CstObjectProp::remove),
            Want::Map(pairs, _) if pairs.is_empty() => prop.into_iter().for_each(CstObjectProp::remove),
            Want::Str(s) => match prop {
                Some(prop) => set_string(&prop, s),
                None => put(entry, None, key, s.as_str().into()),
            },
            Want::List(items) => match prop.as_ref().and_then(CstObjectProp::array_value) {
                Some(list) => sync_list(&list, items),
                None => put(entry, prop, key, strings(items)),
            },
            Want::Map(pairs, syntax) => match (prop.as_ref().and_then(CstObjectProp::object_value), now.get(*key)) {
                (Some(obj), Some(Value::Object(map))) => sync_map(&obj, map, pairs, *syntax),
                _ => put(entry, prop, key, object(pairs)),
            },
        }
    }
}

fn put(entry: &CstObject, prop: Option<CstObjectProp>, key: &str, value: CstInputValue) {
    match prop {
        Some(prop) => prop.set_value(value),
        None => {
            entry.append(key, value);
        }
    }
}

/// Replaces a string in place, so the spacing and comments around it stay.
fn set_string(prop: &CstObjectProp, s: &str) {
    match prop.value().and_then(|v| v.as_string_lit()) {
        Some(lit) => lit.set_raw_value(quoted(s)),
        None => prop.set_value(s.into()),
    }
}

fn sync_list(list: &CstArray, items: &[String]) {
    let elements = list.elements();
    for (i, item) in items.iter().enumerate() {
        match elements.get(i) {
            Some(el) if value_of(el).is_some_and(|v| string(&v, "").is_ok_and(|v| v == *item)) => {}
            Some(el) => match el.as_string_lit() {
                Some(lit) => lit.set_raw_value(quoted(item)),
                None => {
                    list.insert(i, item.as_str().into());
                    el.clone().remove();
                }
            },
            None => {
                list.append(item.as_str().into());
            }
        }
    }
    elements.into_iter().skip(items.len()).for_each(CstNode::remove);
}

fn sync_map(obj: &CstObject, now: &Obj, pairs: &[(String, String)], syntax: Syntax) {
    for prop in obj.properties() {
        if prop.decoded_name().is_some_and(|name| !pairs.iter().any(|(k, _)| *k == name)) {
            prop.remove();
        }
    }
    for (k, text) in pairs {
        match obj.get(k) {
            Some(_) if now.get(k).is_some_and(|v| same_text(v, text, syntax)) => {}
            Some(prop) => set_string(&prop, text),
            None => {
                obj.append(k, text.as_str().into());
            }
        }
    }
}

/// A new entry: the wanted keys in order, then the extras it doesn't already have.
fn fresh(wants: &Wants, extras: &Extras) -> CstInputValue {
    let mut props: Vec<(String, CstInputValue)> = Vec::new();
    for (key, want) in wants {
        let value = match want {
            Want::Str(s) => s.as_str().into(),
            Want::List(items) if !items.is_empty() => strings(items),
            Want::Map(pairs, _) if !pairs.is_empty() => object(pairs),
            _ => continue,
        };
        props.push((key.to_string(), value));
    }
    for (k, v) in extras {
        if !props.iter().any(|(p, _)| p == k) {
            props.push((k.clone(), input(v)));
        }
    }
    CstInputValue::Object(props)
}

fn strings(items: &[String]) -> CstInputValue {
    CstInputValue::Array(items.iter().map(|s| s.as_str().into()).collect())
}

fn object(pairs: &[(String, String)]) -> CstInputValue {
    CstInputValue::Object(pairs.iter().map(|(k, v)| (k.clone(), v.as_str().into())).collect())
}

fn input(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => CstInputValue::Array(a.iter().map(input).collect()),
        Value::Object(o) => CstInputValue::Object(o.iter().map(|(k, v)| (k.clone(), input(v))).collect()),
    }
}

/// A node as JSON, read the way the readers read the file.
fn value_of(node: &impl std::fmt::Display) -> Option<Value> {
    jsonc_parser::parse_to_serde_value(&node.to_string(), &Default::default()).ok()
}

/// A JSON string literal: quotes and control characters escaped, the rest (unicode too) as is.
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}
