//! Exports a chat's history to a text file, with its downloaded files.
//!
//! The transcript mirrors the copy format: `[22:41, 8/18/2026] Ada: message`,
//! with media markers like `[photo]` or `[document: notes.pdf]`. Attachments
//! that were already downloaded land in a `<chat>_files` folder next to the
//! transcript; anything not on disk is marked but skipped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::model::{CallStatus, Chat, Content, Message};

/// What one export produced.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportReport {
    /// The transcript file that was written.
    pub transcript: PathBuf,
    pub messages: usize,
    pub files_copied: usize,
    /// Attachments that were never downloaded, so there was nothing to copy.
    pub files_skipped: usize,
}

/// Makes a chat name safe as a file name on every platform.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|char| {
            if matches!(char, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || char.is_control()
            {
                '_'
            } else {
                char
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "chat".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Writes the chat's transcript and copies its downloaded attachments into
/// `dest`. `dest` is created when missing.
pub fn export_chat(
    archive: &Archive,
    chat: &Chat,
    dest: &Path,
    include_media: bool,
) -> Result<ExportReport, String> {
    std::fs::create_dir_all(dest).map_err(|error| format!("{error:#}"))?;
    let stem = sanitize_filename(&chat.name);
    let transcript_path = dest.join(format!("{stem}.txt"));
    let files_dir = dest.join(format!("{stem}_files"));

    let mut messages = Vec::new();
    let mut before: Option<(i64, String)> = None;
    loop {
        let page = archive
            .messages(
                &chat.id,
                before.as_ref().map(|(time, id)| (*time, id.as_str())),
                500,
            )
            .map_err(|error| format!("{error:#}"))?;
        if page.is_empty() {
            break;
        }
        let oldest = &page[0];
        before = Some((oldest.timestamp, oldest.id.clone()));
        let complete = page.len() < 500;
        // Pages arrive newest-first within themselves but are already
        // ascending; older pages go in front.
        messages.splice(0..0, page);
        if complete {
            break;
        }
    }

    let mut lines = Vec::with_capacity(messages.len() + 3);
    lines.push(format!("Chat: {}", chat.name));
    lines.push(format!(
        "Exported: {}",
        crate::util::copy_stamp(jiff::Timestamp::now().as_second())
    ));
    lines.push(String::new());

    let mut used_names = HashSet::new();
    let mut files_copied = 0;
    let mut files_skipped = 0;
    if include_media {
        std::fs::create_dir_all(&files_dir).map_err(|error| format!("{error:#}"))?;
    }
    for message in &messages {
        let (line, media) = transcript_line(message);
        lines.push(line);
        let Some(source) = media else { continue };
        if !include_media {
            continue;
        }
        match copy_attachment(&source, &files_dir, &mut used_names) {
            Ok(()) => files_copied += 1,
            Err(_) => files_skipped += 1,
        }
    }

    std::fs::write(&transcript_path, lines.join("\n")).map_err(|error| format!("{error:#}"))?;
    Ok(ExportReport {
        transcript: transcript_path,
        messages: messages.len(),
        files_copied,
        files_skipped,
    })
}

/// One transcript line plus the attachment on disk, when the message has one
/// that was already downloaded.
fn transcript_line(message: &Message) -> (String, Option<PathBuf>) {
    let who = message
        .sender_name
        .clone()
        .unwrap_or_else(|| sender_fallback(message));
    let stamp = crate::util::copy_stamp(message.timestamp);
    let (marker, body, media) = match &message.content {
        Content::Text { text, .. } | Content::Interactive { text, .. } => {
            (None, text.clone(), None)
        }
        Content::Image {
            caption, media, ..
        } => (
            Some("[photo]".to_owned()),
            caption.clone().unwrap_or_default(),
            media.path.clone(),
        ),
        Content::Video {
            caption,
            media,
            gif,
            ..
        } => (
            Some(if *gif {
                "[GIF]".to_owned()
            } else {
                "[video]".to_owned()
            }),
            caption.clone().unwrap_or_default(),
            media.path.clone(),
        ),
        Content::Audio {
            media,
            seconds,
            voice_note,
            ..
        } => (
            Some(match (voice_note, seconds) {
                (true, Some(seconds)) => {
                    format!("[voice message, {}]", crate::util::duration(*seconds))
                }
                (true, None) => "[voice message]".to_owned(),
                (false, _) => "[audio]".to_owned(),
            }),
            String::new(),
            media.path.clone(),
        ),
        Content::Document {
            media,
            file_name,
            caption,
            ..
        } => (
            Some(format!("[document: {file_name}]")),
            caption.clone().unwrap_or_default(),
            media.path.clone(),
        ),
        Content::Sticker { media, .. } => (
            Some("[sticker]".to_owned()),
            String::new(),
            media.path.clone(),
        ),
        Content::StickerPack { name, .. } => {
            (Some(format!("[sticker pack: {name}]")), String::new(), None)
        }
        Content::Location { name, .. } => (
            Some("[location]".to_owned()),
            name.clone().unwrap_or_default(),
            None,
        ),
        Content::LiveLocation { ended, .. } => (
            Some(
                if *ended {
                    "[live location ended]"
                } else {
                    "[live location]"
                }
                .to_owned(),
            ),
            String::new(),
            None,
        ),
        Content::Contact { display_name, .. } => (
            Some(format!("[contact: {display_name}]")),
            String::new(),
            None,
        ),
        Content::Poll { question, .. } => {
            (Some(format!("[poll: {question}]")), String::new(), None)
        }
        Content::Revoked => (Some("[message deleted]".to_owned()), String::new(), None),
        Content::Call {
            video,
            status,
            duration,
            ..
        } => {
            let kind = if *video { "video call" } else { "voice call" };
            let status = match status {
                CallStatus::Answered => {
                    if *duration > 0 {
                        format!(
                            "{} ({})",
                            kind,
                            crate::util::duration((*duration).min(u32::MAX as u64) as u32)
                        )
                    } else {
                        kind.to_owned()
                    }
                }
                CallStatus::Missed => format!("missed {kind}"),
                CallStatus::Declined | CallStatus::DeclinedElsewhere => {
                    format!("declined {kind}")
                }
                CallStatus::AnsweredElsewhere => format!("{kind} answered elsewhere"),
                CallStatus::Busy => format!("{kind} (busy)"),
                CallStatus::NoAnswer => format!("unanswered {kind}"),
                CallStatus::ConnectionLost => format!("{kind} (connection lost)"),
                CallStatus::Failed => format!("failed {kind}"),
            };
            (Some(format!("[{status}]")), String::new(), None)
        }
        Content::Unsupported { what } => {
            (Some(format!("[unsupported: {what}]")), String::new(), None)
        }
        Content::PhoneOnly { .. } => (Some("[only on phone]".to_owned()), String::new(), None),
    };
    let mut line = format!("[{stamp}] {who}:");
    if let Some(marker) = marker {
        line.push(' ');
        line.push_str(&marker);
    }
    if !body.is_empty() {
        line.push(' ');
        line.push_str(&body);
    }
    (line, media)
}

/// Display name fallback when WhatsApp never told us the sender's push name.
fn sender_fallback(message: &Message) -> String {
    if message.from_me {
        return "Me".to_owned();
    }
    message
        .sender
        .split('@')
        .next()
        .filter(|phone| !phone.is_empty())
        .unwrap_or(&message.sender)
        .to_owned()
}

/// Copies one attachment into the export folder, de-duplicating file names.
fn copy_attachment(
    source: &Path,
    files_dir: &Path,
    used_names: &mut HashSet<String>,
) -> Result<(), String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let mut candidate = name.to_owned();
    let mut counter = 2;
    while used_names.contains(&candidate) || files_dir.join(&candidate).exists() {
        let stem = Path::new(name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(name);
        let extension = Path::new(name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!(".{extension}"))
            .unwrap_or_default();
        candidate = format!("{stem} ({counter}){extension}");
        counter += 1;
    }
    std::fs::copy(source, files_dir.join(&candidate)).map_err(|error| format!("{error:#}"))?;
    used_names.insert(candidate);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_strips_reserved_characters() {
        assert_eq!(sanitize_filename("Family <Group>"), "Family _Group_");
        assert_eq!(sanitize_filename("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_filename("  ...  "), "chat");
        assert_eq!(sanitize_filename(""), "chat");
        assert_eq!(sanitize_filename("Lunch 🍜"), "Lunch 🍜");
    }

    fn text_message(body: &str) -> Message {
        Message {
            id: "1".to_owned(),
            chat: "chat".to_owned(),
            sender: "123@s.whatsapp.net".to_owned(),
            sender_name: Some("Ada".to_owned()),
            from_me: false,
            timestamp: 1_786_000_000,
            content: Content::Text {
                text: body.to_owned(),
                preview: None,
            },
            status: crate::model::Delivery::Read,
            delivered_at: None,
            read_at: None,
            quoted: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
            forwarded: false,
            thumbnail: None,
            history_order: None,
        }
    }

    #[test]
    fn transcript_line_formats_text() {
        let (line, media) = transcript_line(&text_message("hello"));
        assert!(line.ends_with("Ada: hello"), "{line}");
        assert!(line.starts_with('['), "{line}");
        assert!(media.is_none());
    }

    #[test]
    fn transcript_line_falls_back_to_phone() {
        let mut message = text_message("hi");
        message.sender_name = None;
        let (line, _) = transcript_line(&message);
        assert!(line.contains("123: hi"), "{line}");
        message.from_me = true;
        let (line, _) = transcript_line(&message);
        assert!(line.contains("Me: hi"), "{line}");
    }

    #[test]
    fn copy_attachment_dedupes_names() {
        let root = std::env::temp_dir().join(format!("whatsay-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let files_dir = root.join("files");
        std::fs::create_dir_all(&files_dir).unwrap();
        let source = root.join("photo.jpg");
        std::fs::write(&source, b"data").unwrap();
        let mut used = HashSet::new();
        copy_attachment(&source, &files_dir, &mut used).unwrap();
        copy_attachment(&source, &files_dir, &mut used).unwrap();
        assert!(files_dir.join("photo.jpg").exists());
        assert!(files_dir.join("photo (2).jpg").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
