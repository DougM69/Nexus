//! ADX — ADIF's XML form — read as the ADIF fields it holds, so that an `.adx` file comes to the
//! log through the same field mapping as an `.adi` one.
//!
//! ADIF 3.1.7 §IV (<https://adif.org/317/ADIF_317.htm>, "ADX file format"): a root `ADX` element
//! holding a `HEADER` and a `RECORDS` element, `RECORDS` holding zero or more `RECORD`s, and in a
//! record one element per field, named as the field is (`<CALL>W1AW</CALL>`). Two fields are
//! named by attributes instead, and each maps to the ADIF tag that field has in an `.adi` file:
//!
//! * an application-defined field, `<APP PROGRAMID="P" FIELDNAME="F" TYPE="t">v</APP>`, is
//!   `APP_P_F` (the TYPE indicator is not part of the name, as in `<APP_P_F:1:t>`);
//! * a user-defined field, `<USERDEF FIELDNAME="F">v</USERDEF>`, is `F` (the header defines it;
//!   a record names it, and "If a Data Type Indicator is included in a user-defined field in a
//!   Record, it is ignored").
//!
//! The reader is a small one on purpose: an ADX field is one element holding text, and the five
//! predefined entities plus character references are the whole of the escaping. XML line endings
//! are read as `\n`, as an XML processor reads them. Anything else in a record — an element
//! inside a field, a field left open — is noted ([`NoteKind::BadXml`]), never guessed at.

use super::{header_unmapped, Note, NoteKind, SourceRecord, Unmapped, UnmappedReason};

/// The elements that are the file's structure, never a field's name.
const STRUCTURE: &[&str] = &["ADX", "HEADER", "RECORDS", "RECORD"];

/// One tag: its name (upper-cased), its attributes, and whether it closes or is empty.
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    empty: bool,
}

/// The records an ADX text holds, what was noticed, and what it holds that is not a contact's
/// field (the header's fields, a field name ADIF cannot carry).
pub(crate) fn read(text: &str) -> (Vec<SourceRecord>, Vec<Note>, Vec<Unmapped>) {
    let mut records = Vec::new();
    let mut notes = Vec::new();
    let mut header = Vec::new();
    let mut bad_names: Vec<Unmapped> = Vec::new();
    // Where we are: inside HEADER, inside a RECORD (its fields so far), inside a field (its name
    // and text so far).
    let mut in_header = false;
    let mut record: Option<Vec<(String, String)>> = None;
    let mut field: Option<(String, String, String)> = None; // (element, ADIF name, text)
    let mut pos = 0;
    while pos < text.len() {
        let Some(lt) = text[pos..].find('<').map(|i| pos + i) else {
            break;
        };
        if let Some((_, _, t)) = field.as_mut() {
            t.push_str(&unescape(&text[pos..lt]));
        }
        let rest = &text[lt..];
        let skip_to = |end: &str| rest.find(end).map(|i| lt + i + end.len());
        if rest.starts_with("<!--") {
            pos = skip_to("-->").unwrap_or(text.len());
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map_or(text.len(), |i| lt + i);
            if let Some((_, _, t)) = field.as_mut() {
                t.push_str(&rest[9..end - lt].replace("\r\n", "\n"));
            }
            pos = (end + 3).min(text.len());
            continue;
        }
        if rest.starts_with("<?") {
            pos = skip_to("?>").unwrap_or(text.len());
            continue;
        }
        if rest.starts_with("<!") {
            pos = skip_to(">").unwrap_or(text.len());
            continue;
        }
        let Some((tag, len)) = parse_tag(rest) else {
            notes.push(Note {
                kind: NoteKind::BadXml,
                at: records.len() + 1,
                text: super::clip(rest),
            });
            break;
        };
        pos = lt + len;
        let at = records.len() + 1;
        if let Some((element, name, value)) = field.take() {
            if tag.closing && tag.name == element {
                push(record.as_mut().unwrap_or(&mut header), name, value);
                continue;
            }
            if !STRUCTURE.contains(&tag.name.as_str()) {
                notes.push(Note {
                    kind: NoteKind::BadXml,
                    at,
                    text: format!("<{}> in <{element}>", tag.name),
                });
                field = Some((element, name, value));
                continue;
            }
            // The file's own structure ends a field left open, so that it never runs on into the
            // next record: the field is let go, said, and the tag read as what it is.
            notes.push(Note {
                kind: NoteKind::BadXml,
                at,
                text: format!("<{element}> not closed"),
            });
        }
        match (tag.name.as_str(), tag.closing) {
            ("HEADER", false) => in_header = !tag.empty,
            ("HEADER", true) => in_header = false,
            ("RECORD", false) => {
                // A record not closed before the next one opens ends there.
                if let Some(fields) = record.take() {
                    notes.push(Note {
                        kind: NoteKind::BadXml,
                        at,
                        text: "<RECORD> not closed".to_string(),
                    });
                    records.push(SourceRecord { at, fields });
                }
                let at = records.len() + 1;
                if tag.empty {
                    records.push(SourceRecord {
                        at,
                        fields: Vec::new(),
                    });
                } else {
                    record = Some(Vec::new());
                }
            }
            ("RECORD", true) => {
                if let Some(fields) = record.take() {
                    records.push(SourceRecord { at, fields });
                }
            }
            (n, false) if (record.is_some() || in_header) && !STRUCTURE.contains(&n) => {
                // A header's USERDEF defines a field (by FIELDID) rather than holding one: listed
                // under its element's own name.
                let name = if record.is_some() {
                    field_name(&tag)
                } else {
                    tag.name.clone()
                };
                if name.is_empty() || name.contains([':', '<', '>', ' ', ',']) {
                    match bad_names.iter_mut().find(|u| u.what == name) {
                        Some(u) => u.count += 1,
                        None => bad_names.push(Unmapped {
                            what: name.clone(),
                            side: None,
                            count: 1,
                            reason: UnmappedReason::FieldName,
                        }),
                    }
                    if !tag.empty {
                        // Read its text and let it go: it has nowhere to be stored.
                        field = Some((tag.name, String::new(), String::new()));
                    }
                } else if tag.empty {
                    push(record.as_mut().unwrap_or(&mut header), name, String::new());
                } else {
                    field = Some((tag.name, name, String::new()));
                }
            }
            _ => {}
        }
    }
    if let Some(fields) = record {
        notes.push(Note {
            kind: NoteKind::BadXml,
            at: records.len() + 1,
            text: "<RECORD> not closed".to_string(),
        });
        records.push(SourceRecord {
            at: records.len() + 1,
            fields,
        });
    }
    // A field whose name was refused is read and discarded: `push` drops an empty name.
    records
        .iter_mut()
        .for_each(|r| r.fields.retain(|(k, _)| !k.is_empty()));
    header.retain(|(k, _)| !k.is_empty());
    let mut unmapped = header_unmapped(&header);
    unmapped.extend(bad_names);
    (records, notes, unmapped)
}

fn push(fields: &mut Vec<(String, String)>, name: String, value: String) {
    fields.push((name, value));
}

/// The ADIF tag an element is the field of.
fn field_name(tag: &Tag) -> String {
    let attr = |n: &str| {
        tag.attrs
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.trim().to_ascii_uppercase())
            .unwrap_or_default()
    };
    match tag.name.as_str() {
        "APP" => format!("APP_{}_{}", attr("PROGRAMID"), attr("FIELDNAME")),
        "USERDEF" => attr("FIELDNAME"),
        n => n.to_string(),
    }
}

/// The tag `s` starts with, and its length. A `>` inside a quoted attribute value does not end it.
fn parse_tag(s: &str) -> Option<(Tag, usize)> {
    let mut quote: Option<char> = None;
    let mut end = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                end = Some(i);
                break;
            }
            (None, '<') => return None,
            _ => {}
        }
    }
    let end = end?;
    let mut inner = &s[1..end];
    let closing = inner.starts_with('/');
    if closing {
        inner = &inner[1..];
    }
    let empty = inner.ends_with('/');
    if empty {
        inner = &inner[..inner.len() - 1];
    }
    let inner = inner.trim();
    let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_uppercase();
    let mut attrs = Vec::new();
    let mut rest = inner[name_end..].trim_start();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_ascii_uppercase();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(q) else {
            break;
        };
        attrs.push((key, unescape(&after[1..1 + close])));
        rest = after[close + 2..].trim_start();
    }
    if name.is_empty() {
        return None;
    }
    Some((
        Tag {
            name,
            attrs,
            closing,
            empty,
        },
        end + 1,
    ))
}

/// XML text as the characters it stands for: the five predefined entities, decimal and hex
/// character references, and `\r\n` read as `\n`. An entity this reader does not know is kept as
/// it was written.
fn unescape(s: &str) -> String {
    if !s.contains('&') && !s.contains('\r') {
        return s.to_string();
    }
    let s = s.replace("\r\n", "\n");
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        // The `;` looked for only as far as the longest reference read: a search to the end of the
        // text for each `&` would make a run of them quadratic.
        let semi = after.as_bytes().iter().take(11).position(|&b| b == b';');
        let decoded = semi.and_then(|semi| {
            let name = &after[1..semi];
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
                    .ok()
                    .and_then(char::from_u32),
                n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            }?;
            Some((c, semi + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &after[len..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
