//! A DeleteObjects document read: the one reader of it every module uses.

/// An object a DeleteObjects document names: its key, and the version
/// named, if one is.
pub type Deleted = (String, Option<String>);

/// The objects a DeleteObjects document names, as a store would read them:
/// entities resolved (`a&amp;b` is `a&b`), names with a prefix (`s3:Key`)
/// by their local name, and what else it holds passed over. A document cut
/// short (a record keeps the first [`crate::MAX_REQUEST_BODY`] bytes) names
/// the objects it holds whole. What a run did, not what a guard lets by:
/// that is [`deleted_objects_strict`].
pub fn deleted_objects(doc: &str) -> Vec<Deleted> {
    read(doc, false).unwrap_or_default()
}

/// [`deleted_objects`] for a guard: none when the document does not read
/// cleanly or holds anything but `Delete`, its `Object`s (each one `Key`, at
/// most one `VersionId`, `ETag`, `LastModifiedTime`, `Size`) and `Quiet`,
/// so a store never removes an object the guard did not see. No prefixes,
/// attributes (but the S3 namespace on `Delete`), comments, CDATA, doctype
/// or entities of its own.
pub fn deleted_objects_strict(doc: &str) -> Option<Vec<Deleted>> {
    read(doc, true)
}

fn read(doc: &str, strict: bool) -> Option<Vec<Deleted>> {
    use quick_xml::events::Event;
    const NS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";
    const FIELDS: [&str; 5] = ["Key", "VersionId", "ETag", "LastModifiedTime", "Size"];
    let space = |t: &str| t.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    let mut reader = quick_xml::Reader::from_str(doc);
    let mut open: Vec<String> = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new(); // the open Object's
    let mut text = String::new(); // the open field's
    let mut out = Vec::new();
    let (mut first, mut rooted, mut quiet) = (true, false, false);
    // (read leniently: how deep in an element not of the document's shape)
    let mut skipped = 0usize;
    loop {
        // a field (Delete/Object/Key) or Quiet (Delete/Quiet) holds text
        let leaf = skipped == 0 && (open.len() == 3 || open.last().is_some_and(|n| n == "Quiet"));
        let Ok(event) = reader.read_event() else {
            // cut short or torn: what was read whole is what it names
            return (!strict).then_some(out);
        };
        match event {
            Event::Decl(_) if first => {}
            Event::Start(_) if skipped > 0 => skipped += 1,
            Event::Start(e) => {
                let name = if strict {
                    e.name().0
                } else {
                    e.local_name().into_inner()
                };
                let fits = match open.len() {
                    0 => name == "Delete" && !rooted,
                    1 => name == "Object" || (name == "Quiet" && !quiet),
                    2 => {
                        open[1] == "Object"
                            && FIELDS.contains(&name)
                            && !fields.iter().any(|(n, _)| n == name)
                    }
                    _ => false,
                };
                if !fits {
                    if strict {
                        return None;
                    }
                    skipped = 1;
                } else {
                    if strict {
                        for a in e.attributes() {
                            let a = a.ok()?;
                            if !(open.is_empty() && a.key.0 == "xmlns" && a.value == NS) {
                                return None;
                            }
                        }
                    }
                    rooted = true;
                    quiet |= name == "Quiet";
                    open.push(name.to_string());
                    text.clear();
                }
            }
            Event::End(_) if skipped > 0 => skipped -= 1,
            Event::End(_) => {
                // the reader checks the name matches
                let name = open.pop()?;
                if open.len() == 2 {
                    fields.push((name, std::mem::take(&mut text)));
                } else if name == "Object" {
                    let get = |n: &str| fields.iter().find(|(f, _)| f == n).map(|(_, v)| v.clone());
                    match get("Key") {
                        Some(key) => out.push((key, get("VersionId"))),
                        None if strict => return None,
                        None => {}
                    }
                    fields.clear();
                }
            }
            Event::Text(t) if leaf => text.push_str(&t.xml10_content()),
            Event::Text(t) if space(&t) => {}
            Event::CData(c) if leaf && !strict => text.push_str(&c.into_inner()),
            Event::GeneralRef(r) if leaf => match r.resolve_char_ref().ok()? {
                Some(c) => text.push(c),
                None => match quick_xml::escape::resolve_xml_entity(&r) {
                    Some(e) => text.push_str(e),
                    None if strict => return None,
                    None => {}
                },
            },
            Event::Eof if strict => {
                return (rooted && open.is_empty() && !out.is_empty()).then_some(out);
            }
            Event::Eof => return Some(out),
            _ if strict => return None,
            _ => {}
        }
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(doc: &str) -> Vec<Deleted> {
        deleted_objects(doc)
    }

    #[test]
    fn a_document_names_its_keys_and_versions() {
        let doc = "<?xml version=\"1.0\"?>\n<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                   <Quiet>true</Quiet><Object><Key>a&amp;b</Key><VersionId>v1</VersionId></Object>\
                   <Object><Key>dir/</Key></Object></Delete>";
        let want = vec![
            ("a&b".to_string(), Some("v1".to_string())),
            ("dir/".into(), None),
        ];
        assert_eq!(keys(doc), want);
        assert_eq!(deleted_objects_strict(doc), Some(want));
    }

    #[test]
    fn read_leniently_a_document_is_read_as_a_store_reads_it() {
        // a prefix, CDATA, a field of its own, a character reference
        let doc = "<s3:Delete xmlns:s3=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                   <s3:Object><s3:Key>&#109;ine</s3:Key><Extra>x</Extra></s3:Object>\
                   <Object><Key><![CDATA[c<d]]></Key></Object></s3:Delete>";
        assert_eq!(
            keys(doc),
            vec![("mine".to_string(), None), ("c<d".to_string(), None)]
        );
        assert_eq!(deleted_objects_strict(doc), None);
        // not the text of a Key elsewhere, nor `</Key>` written in one
        assert_eq!(
            keys("<Delete><Object><Key>a/&lt;/Key&gt;</Key><ETag>/</ETag></Object></Delete>"),
            vec![("a/</Key>".to_string(), None)]
        );
    }

    #[test]
    fn a_document_cut_short_names_what_it_holds_whole() {
        let doc = "<Delete><Object><Key>a</Key></Object><Object><Key>b</Key></Object><Object><Ke";
        assert_eq!(keys(doc), vec![("a".to_string(), None), ("b".into(), None)]);
        assert_eq!(deleted_objects_strict(doc), None);
        assert!(keys("not xml at all").is_empty());
    }
}
