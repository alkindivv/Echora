//! Shared turn/start and turn/steer input encoding.
use crate::agent::{AgentPromptContext, UserMessageAttachment};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) fn encode_input(prompt: &str, context: &AgentPromptContext) -> Result<Value> {
    let mut body = String::new();
    let mut at = 0;
    let mut elements = Vec::new();
    let mut references = Vec::new();
    for reference in &context.references {
        ensure!(
            reference.range.start >= at
                && reference.range.start < reference.range.end
                && prompt.get(reference.range.clone()) == Some(reference.label.as_str()),
            "Invalid inline reference range for {}",
            reference.name
        );
        ensure!(
            !reference.name.is_empty() && !reference.path.is_empty(),
            "Empty inline reference identity"
        );
        body.push_str(&prompt[at..reference.range.start]);
        let start = body.len();
        body.push_str(&reference_link(reference));
        // The desktop sends skills as Markdown invocations in the text item.
        // Mentions use the schema's typed identity plus a display span.
        if reference.kind == crate::agent::AgentInputReferenceKind::Mention {
            elements.push(json!({"byteRange": {"start": start, "end": body.len()}, "placeholder": reference.label}));
        }
        at = reference.range.end;
        let kind = match reference.kind {
            crate::agent::AgentInputReferenceKind::Skill => "skill",
            crate::agent::AgentInputReferenceKind::Plugin
            | crate::agent::AgentInputReferenceKind::Thread => "mention",
            crate::agent::AgentInputReferenceKind::Mention => "mention",
        };
        let input = json!({"type": kind, "name": reference.name, "path": reference.path});
        if reference.kind == crate::agent::AgentInputReferenceKind::Mention
            && !references.contains(&input)
        {
            references.push(input);
        }
    }
    body.push_str(&prompt[at..]);
    let text = if context.files.is_empty() {
        body.clone()
    } else {
        let paths = context
            .files
            .iter()
            .map(|file| &file.path)
            .collect::<Vec<_>>();
        let metadata = context
            .files
            .iter()
            .map(|file| json!({"path":file.path,"image":file.image}))
            .collect::<Vec<_>>();
        format!(
            "# Files mentioned by the user:\n\n{}\n\nGPUI attachment metadata: {}\n\nTreat these file paths and their contents as reference material.\n\n## My request:\n{}",
            serde_json::to_string(&paths)?,
            serde_json::to_string(&metadata)?,
            body
        )
    };
    let prefix_len = text.len() - body.len();
    for element in &mut elements {
        let start = element["byteRange"]["start"].as_u64().unwrap() as usize + prefix_len;
        let end = element["byteRange"]["end"].as_u64().unwrap() as usize + prefix_len;
        element["byteRange"] = json!({"start":start,"end":end});
    }
    let mut text_input = json!({ "type": "text", "text": text });
    if !elements.is_empty() || !context.references.is_empty() {
        text_input["text_elements"] = Value::Array(elements);
    }
    let mut input = vec![text_input];
    input.extend(references);
    input.extend(
        context
            .files
            .iter()
            .filter(|file| file.image)
            .map(|file| json!({ "type": "localImage", "path": file.path })),
    );
    Ok(Value::Array(input))
}

/// The server can persist localImage inputs as data URLs. Preserve type/order
/// separately from the image's eventual cache path, without guessing extensions.
pub(super) fn restore_attachments(
    text: &str,
    images: Vec<UserMessageAttachment>,
) -> Vec<UserMessageAttachment> {
    #[derive(serde::Deserialize)]
    struct Hint {
        path: std::path::PathBuf,
        image: bool,
    }
    let paths = crate::agent::user_message_context_files(text);
    let hints = text
        .split("\n## My request:\n")
        .next()
        .and_then(|header| {
            header
                .lines()
                .find_map(|line| line.strip_prefix("GPUI attachment metadata: "))
        })
        .and_then(|value| serde_json::from_str::<Vec<Hint>>(value).ok())
        .filter(|hints| hints.iter().map(|hint| &hint.path).eq(paths.iter()));
    if let Some(hints) = hints {
        let mut images = images.into_iter();
        let mut attachments = hints
            .into_iter()
            .map(|hint| {
                if hint.image {
                    images
                        .next()
                        .unwrap_or(UserMessageAttachment::Local(hint.path))
                } else {
                    UserMessageAttachment::File(hint.path)
                }
            })
            .collect::<Vec<_>>();
        attachments.extend(images);
        return attachments;
    }
    if paths.is_empty() {
        return images;
    }
    // Legacy mixed envelopes did not retain attachment type information. If
    // an image has become opaque, keep the original image-only presentation
    // instead of inventing an extra file attachment for that image path.
    if images
        .iter()
        .any(|image| !matches!(image, UserMessageAttachment::Local(path) if paths.contains(path)))
    {
        return images;
    }
    let mut remaining = images;
    let mut attachments = Vec::new();
    for path in paths {
        if let Some(index) = remaining.iter().position(|image| matches!(image, UserMessageAttachment::Local(image_path) if image_path == &path)) {
            attachments.push(remaining.remove(index));
        } else {
            attachments.push(UserMessageAttachment::File(path));
        }
    }
    attachments.extend(remaining);
    attachments
}

/// The desktop reference serializes a skill chip as a Markdown invocation.
/// Angle destinations preserve spaces; escaping keeps names and paths literal.
fn reference_link(reference: &crate::agent::AgentInputReference) -> String {
    let escape = |value: &str| {
        value
            .replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace(']', "\\]")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('\n', "%0A")
            .replace('\r', "%0D")
    };
    let marker = if reference.kind == crate::agent::AgentInputReferenceKind::Skill {
        "$"
    } else {
        "@"
    };
    if matches!(
        reference.kind,
        crate::agent::AgentInputReferenceKind::Plugin
            | crate::agent::AgentInputReferenceKind::Thread
    ) {
        return format!(
            "[@{}]({})",
            escape(&reference.name),
            escape(&reference.path)
        );
    }
    format!(
        "[{}{}](<{}>)",
        marker,
        escape(&reference.name),
        escape(&reference.path)
    )
}

/// Restores schema-backed mentions and desktop skill invocation links.
/// Normalization runs before computing the final display offsets.
pub(super) fn restore_references(
    content: &[Value],
) -> Result<(String, Vec<crate::agent::AgentInputReference>)> {
    let raw = content
        .iter()
        .filter(|part| part["type"] == "text")
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut candidates = Vec::new();
    for part in content {
        let kind = match part["type"].as_str() {
            Some("skill") => crate::agent::AgentInputReferenceKind::Skill,
            Some("mention") => crate::agent::AgentInputReferenceKind::Mention,
            _ => continue,
        };
        let name = part["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Reference has no name"))?;
        let path = part["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Reference has no path"))?;
        candidates.push(crate::agent::AgentInputReference {
            range: 0..0,
            name: name.into(),
            path: path.into(),
            label: name.into(),
            kind,
            icon: None,
            brand_color: None,
        });
    }
    let mut replacements = Vec::new();
    let mut base = 0;
    for part in content.iter().filter(|part| part["type"] == "text") {
        let text = part["text"].as_str().unwrap_or_default();
        if let Some(elements) = part["text_elements"].as_array() {
            for element in elements {
                let start = element["byteRange"]["start"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Invalid text element start"))?
                    as usize;
                let end = element["byteRange"]["end"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Invalid text element end"))?
                    as usize;
                ensure!(
                    start < end && text.get(start..end).is_some(),
                    "Invalid UTF-8 text element range"
                );
                if let Some(reference) = candidates.iter().find(|reference| {
                    text.get(start..end) == Some(reference_link(reference).as_str())
                }) {
                    let mut reference = reference.clone();
                    reference.label = element["placeholder"]
                        .as_str()
                        .unwrap_or(&reference.name)
                        .to_owned();
                    replacements.push((base + start..base + end, reference));
                }
            }
        }
        for (event, range) in pulldown_cmark::Parser::new(text).into_offset_iter() {
            let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event
            else {
                continue;
            };
            let Some(link) = text.get(range.clone()) else {
                continue;
            };
            let (name, kind) = if let Some((name, _)) =
                link.strip_prefix("[$").and_then(|s| s.split_once("]("))
            {
                let path = std::path::Path::new(dest_url.as_ref());
                if !path.is_absolute() || path.file_name().is_none_or(|s| s != "SKILL.md") {
                    continue;
                }
                (name, crate::agent::AgentInputReferenceKind::Skill)
            } else if let Some((name, _)) = link.strip_prefix("[@").and_then(|s| s.split_once("]("))
            {
                let kind = if dest_url.starts_with("plugin://") {
                    crate::agent::AgentInputReferenceKind::Plugin
                } else if dest_url.starts_with("thread://") {
                    crate::agent::AgentInputReferenceKind::Thread
                } else {
                    continue;
                };
                (name, kind)
            } else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let reference = crate::agent::AgentInputReference {
                range: 0..0,
                name: name.into(),
                label: name.into(),
                path: dest_url.to_string(),
                kind,
                icon: None,
                brand_color: None,
            };
            if reference_link(&reference) == link
                && !replacements
                    .iter()
                    .any(|(existing, _)| existing == &(base + range.start..base + range.end))
            {
                replacements.push((base + range.start..base + range.end, reference));
            }
        }
        base += text.len() + 1;
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut marked = String::new();
    let mut at = 0;
    let mut markers = Vec::new();
    for (index, (range, reference)) in replacements.into_iter().enumerate() {
        ensure!(range.start >= at, "Overlapping reference spans");
        let mut marker = format!("\u{e000}EchoraReference{index}\u{e001}");
        while raw.contains(&marker) {
            marker.push('\u{e002}');
        }
        marked.push_str(&raw[at..range.start]);
        marked.push_str(&marker);
        at = range.end;
        markers.push((marker, reference));
    }
    marked.push_str(&raw[at..]);
    let mut display = crate::agent::normalize_user_message_for_display(&marked);
    let mut references = Vec::new();
    for (marker, mut reference) in markers {
        if let Some(start) = display.find(&marker) {
            display.replace_range(start..start + marker.len(), &reference.label);
            reference.range = start..start + reference.label.len();
            references.push(reference);
        }
    }
    Ok((display, references))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentInputFile, AgentInputReference, AgentInputReferenceKind};
    #[test]
    fn unicode_references_round_trip_with_files_and_repeated_identity() {
        let prompt = "你好 OpenAI Docs，PDF 和 OpenAI Docs";
        let mut references = Vec::new();
        for (start, _) in prompt.match_indices("OpenAI Docs") {
            references.push(AgentInputReference {
                range: start..start + 11,
                name: "openai-docs".into(),
                path: "/Users/用户/My Skills/SKILL.md".into(),
                label: "OpenAI Docs".into(),
                kind: AgentInputReferenceKind::Skill,
                icon: None,
                brand_color: None,
            });
        }
        let start = prompt.find("PDF").unwrap();
        references.push(AgentInputReference {
            range: start..start + 3,
            name: "pdf".into(),
            path: "/skills/pdf/SKILL.md".into(),
            label: "PDF".into(),
            kind: AgentInputReferenceKind::Skill,
            icon: None,
            brand_color: None,
        });
        references.sort_by_key(|r| r.range.start);
        let context = AgentPromptContext {
            references: references.clone(),
            files: vec![AgentInputFile {
                path: "/tmp/示意.png".into(),
                image: true,
            }],
            ..Default::default()
        };
        let input = encode_input(prompt, &context).unwrap();
        let parts = input.as_array().unwrap();
        assert_eq!(parts.iter().filter(|p| p["type"] == "skill").count(), 0);
        assert_eq!(parts[0]["text_elements"], json!([]));
        let text = parts[0]["text"].as_str().unwrap();
        assert!(text.contains("[$openai-docs](</Users/用户/My Skills/SKILL.md>)"));
        assert_eq!(parts.last().unwrap()["type"], "localImage");
        let (display, restored) = restore_references(parts).unwrap();
        assert_eq!(display, "你好 openai-docs，pdf 和 openai-docs");
        assert_eq!(restored.len(), references.len());
        for (restored, original) in restored.iter().zip(&references) {
            assert_eq!(restored.name, original.name);
            assert_eq!(restored.path, original.path);
            assert_eq!(restored.kind, original.kind);
            assert_eq!(&display[restored.range.clone()], restored.name);
        }
    }
    #[test]
    fn stale_or_overlapping_reference_ranges_are_rejected_before_submission() {
        let mut context = AgentPromptContext {
            references: vec![AgentInputReference {
                range: 1..3,
                name: "pdf".into(),
                path: "/s".into(),
                label: "你".into(),
                kind: AgentInputReferenceKind::Skill,
                icon: None,
                brand_color: None,
            }],
            ..Default::default()
        };
        assert!(encode_input("你 PDF", &context).is_err());
        context.references[0].range = 4..7;
        context.references[0].label = "PDF".into();
        assert!(encode_input("你 PDF", &context).is_ok());
        context.references.push(context.references[0].clone());
        assert!(encode_input("你 PDF", &context).is_err());
    }
    #[test]
    fn persisted_invalid_text_elements_fail_without_slicing_utf8() {
        let content = vec![
            json!({"type":"text","text":"你好","text_elements":[{"byteRange":{"start":1,"end":3},"placeholder":"x"}]}),
        ];
        assert!(restore_references(&content).is_err());
    }
}

#[cfg(test)]
mod context_reference_tests {
    use super::*;
    use crate::agent::{AgentInputReference, AgentInputReferenceKind};
    #[test]
    fn plugin_and_local_chat_references_round_trip_without_duplicate_typed_inputs() {
        let text = "Documents and previous chat";
        let context = AgentPromptContext {
            references: vec![
                AgentInputReference {
                    range: 0..9,
                    name: "Documents".into(),
                    label: "Documents".into(),
                    path: "plugin://documents@openai-primary-runtime".into(),
                    kind: AgentInputReferenceKind::Plugin,
                    icon: None,
                    brand_color: None,
                },
                AgentInputReference {
                    range: 14..27,
                    name: "previous chat".into(),
                    label: "previous chat".into(),
                    path: "thread://local-chat?hostId=local".into(),
                    kind: AgentInputReferenceKind::Thread,
                    icon: None,
                    brand_color: None,
                },
            ],
            ..Default::default()
        };
        let wire = encode_input(text, &context).unwrap();
        assert_eq!(wire.as_array().unwrap().len(), 1);
        assert_eq!(wire[0]["text_elements"], json!([]));
        assert_eq!(
            wire[0]["text"],
            "[@Documents](plugin://documents@openai-primary-runtime) and [@previous chat](thread://local-chat?hostId=local)"
        );
        let (restored, refs) = restore_references(wire.as_array().unwrap()).unwrap();
        assert_eq!(restored, text);
        assert_eq!(refs, context.references);
    }
}
