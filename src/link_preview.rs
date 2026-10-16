//! Page previews for web links in conversation Markdown.
//!
//! ChatGPT opens a card under a hovered web link: the host and an `Open`
//! action first, then the page's Open Graph image, title and description once
//! they load. This application has no HTTP client, so the page head and the
//! preview image are fetched with the system `curl`, as pull request avatars
//! are; images are cached on disk so a link hovered again draws at once.

use std::{fs, path::PathBuf, process::Command, time::Duration};

use anyhow::{Result, bail};

use crate::git_review::process;

const FETCH_TIMEOUT: Duration = Duration::from_secs(12);
/// Open Graph tags live in the document head; a larger page is cut off rather
/// than read whole.
const MAX_PAGE_BYTES: &str = "1048576";
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Echora";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
}

impl PageMetadata {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none() && self.image_url.is_none()
    }
}

/// The host a card names: `www.` is dropped, as the reference prints it.
pub fn display_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_owned())
}

/// Downloads the page at `url` and reads its preview metadata.
pub fn fetch_metadata(url: &str) -> Result<PageMetadata> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("only web pages have previews");
    }
    let mut command = Command::new("curl");
    command
        .args([
            "--silent",
            "--location",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--globoff",
            "--max-redirs",
            "5",
            "--max-time",
            "10",
            "--range",
            "0-1048575",
            "--max-filesize",
            MAX_PAGE_BYTES,
            "--user-agent",
            USER_AGENT,
            "--header",
            "Accept: text/html,application/xhtml+xml",
            "--",
        ])
        .arg(url);
    let output = process::run(&mut command, None, FETCH_TIMEOUT)?;
    // `--max-filesize` stops a large page with an error, but a page without a
    // length header still arrives cut off; whatever head it carries is enough.
    if output.stdout.is_empty() {
        bail!("curl exited with {}", output.status);
    }
    let html = String::from_utf8_lossy(&output.stdout);
    Ok(parse_metadata(&html, url))
}

fn cache_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("GPUI_LINK_PREVIEW_CACHE") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Caches/GPUI/link-previews");
    }
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache).join("gpui/link-previews");
    }
    std::env::temp_dir().join("gpui-link-previews")
}

/// FNV-1a, so a URL keeps its cache file across builds.
fn file_name(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Downloads a preview image into the cache (once) and returns the file.
pub fn fetch_image(url: &str) -> Result<PathBuf> {
    let directory = cache_dir();
    let path = directory.join(file_name(url));
    if path.is_file() {
        return Ok(path);
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!("only web images are fetched");
    }
    fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!("{}.{}.part", file_name(url), std::process::id()));
    let mut command = Command::new("curl");
    command
        .args([
            "--fail",
            "--silent",
            "--location",
            "--proto",
            "=http,https",
            "--proto-redir",
            "=http,https",
            "--globoff",
            "--max-time",
            "10",
            "--max-filesize",
            "8388608",
            "--user-agent",
            USER_AGENT,
            "--output",
        ])
        .arg(&temporary)
        .arg("--")
        .arg(url);
    let output = process::run(&mut command, None, FETCH_TIMEOUT)?;
    if !output.status.success() {
        let _ = fs::remove_file(&temporary);
        bail!("curl exited with {}", output.status);
    }
    let bytes = fs::read(&temporary)?;
    if crate::media::encoded_image_dimensions(&bytes).is_none() && !is_webp(&bytes) {
        let _ = fs::remove_file(&temporary);
        bail!("the preview is not an image");
    }
    fs::rename(&temporary, &path)?;
    Ok(path)
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

/// Reads Open Graph (then Twitter, then plain HTML) title, description and
/// image from a page head; a relative image resolves against `page_url`.
pub fn parse_metadata(html: &str, page_url: &str) -> PageMetadata {
    let head = match find_ascii_case_insensitive(html, "</head") {
        Some(end) => &html[..end],
        None => html,
    };
    let mut og_title = None;
    let mut og_description = None;
    let mut og_image = None;
    let mut twitter_title = None;
    let mut twitter_description = None;
    let mut twitter_image = None;
    let mut description = None;
    let mut rest = head;
    while let Some(start) = find_ascii_case_insensitive(rest, "<meta") {
        let tag_start = &rest[start..];
        let Some(end) = tag_end(tag_start) else {
            break;
        };
        let tag = &tag_start[..end];
        rest = &tag_start[end + 1..];
        if !tag.as_bytes().get(5).is_some_and(u8::is_ascii_whitespace) {
            continue;
        }
        let key = attribute(tag, "property")
            .or_else(|| attribute(tag, "name"))
            .map(|key| key.to_ascii_lowercase());
        let Some(content) = attribute(tag, "content")
            .map(|content| decode_entities(&content).trim().to_owned())
            .filter(|content| !content.is_empty())
        else {
            continue;
        };
        let slot = match key.as_deref() {
            Some("og:title") => &mut og_title,
            Some("og:description") => &mut og_description,
            Some("og:image" | "og:image:url" | "og:image:secure_url") => &mut og_image,
            Some("twitter:title") => &mut twitter_title,
            Some("twitter:description") => &mut twitter_description,
            Some("twitter:image" | "twitter:image:src") => &mut twitter_image,
            Some("description") => &mut description,
            _ => continue,
        };
        slot.get_or_insert(content);
    }
    let title = og_title.or(twitter_title).or_else(|| document_title(head));
    let image_url = og_image.or(twitter_image).and_then(|image| {
        url::Url::parse(page_url)
            .ok()?
            .join(&image)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(String::from)
    });
    PageMetadata {
        title,
        description: og_description.or(twitter_description).or(description),
        image_url,
    }
}

fn document_title(head: &str) -> Option<String> {
    let start = find_ascii_case_insensitive(head, "<title")?;
    let open_end = head[start..].find('>')? + start + 1;
    let close = find_ascii_case_insensitive(&head[open_end..], "</title")? + open_end;
    let title = decode_entities(&head[open_end..close]);
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then_some(title)
}

fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in tag.bytes().enumerate() {
        match (quote, byte) {
            (Some(current), byte) if current == byte => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(index),
            _ => {}
        }
    }
    None
}

/// The value of `name` in the attribute list of one tag, quoted or not.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut search = 0;
    while let Some(found) = find_ascii_case_insensitive(&tag[search..], name) {
        let index = search + found;
        search = index + name.len();
        let preceded = index
            .checked_sub(1)
            .map(|before| bytes[before].is_ascii_whitespace())
            .unwrap_or(false);
        let mut cursor = index + name.len();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if !preceded || bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        return match bytes.get(cursor) {
            Some(&quote @ (b'"' | b'\'')) => {
                let value_start = cursor + 1;
                let value_end = tag[value_start..]
                    .find(quote as char)
                    .map_or(tag.len(), |end| value_start + end);
                Some(tag[value_start..value_end].to_owned())
            }
            Some(_) => {
                let value_end = tag[cursor..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '>')
                    .map_or(tag.len(), |end| cursor + end);
                Some(tag[cursor..value_end].to_owned())
            }
            None => None,
        };
    }
    None
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let entity = &rest[start..];
        let Some(end) = entity.find(';').filter(|end| *end <= 10) else {
            decoded.push('&');
            rest = &entity[1..];
            continue;
        };
        let name = &entity[1..end];
        let character = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match character {
            Some(character) => {
                decoded.push(character);
                rest = &entity[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &entity[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_graph_tags_win_over_plain_html() {
        let html = r#"<html><head>
            <title>Plain &amp; simple</title>
            <meta name="description" content="Plain description">
            <meta property="og:title" content="GitHub - c-narcissus/figure: A skill &#39;for&#39; figures">
            <meta property='og:description' content='Open Graph description'>
            <meta property="og:image" content="/assets/card.png" />
            </head><body><meta property="og:title" content="ignored"></body></html>"#;
        let metadata = parse_metadata(html, "https://github.com/c-narcissus/figure#readme");
        assert_eq!(
            metadata.title.as_deref(),
            Some("GitHub - c-narcissus/figure: A skill 'for' figures")
        );
        assert_eq!(
            metadata.description.as_deref(),
            Some("Open Graph description")
        );
        assert_eq!(
            metadata.image_url.as_deref(),
            Some("https://github.com/assets/card.png")
        );
    }

    #[test]
    fn plain_html_fills_in_when_open_graph_is_missing() {
        let html = "<HEAD><TITLE>\n  Example   Domain \n</TITLE><META NAME=description CONTENT=Short></HEAD>";
        let metadata = parse_metadata(html, "https://example.com/");
        assert_eq!(metadata.title.as_deref(), Some("Example Domain"));
        assert_eq!(metadata.description.as_deref(), Some("Short"));
        assert_eq!(metadata.image_url, None);
    }

    #[test]
    fn metadata_preserves_url_slashes_and_quoted_angle_brackets() {
        let metadata = parse_metadata(
            r#"<head><metadata name=description content=Ignored>
            <meta property=og:image content=https://example.com/card.png>
            <meta property='og:title' content='A > B'></head>"#,
            "https://example.com/",
        );
        assert_eq!(metadata.title.as_deref(), Some("A > B"));
        assert_eq!(
            metadata.image_url.as_deref(),
            Some("https://example.com/card.png")
        );
        assert_eq!(metadata.description, None);
    }

    #[test]
    fn a_page_without_metadata_has_no_preview_body() {
        assert!(parse_metadata("<html><body>zip</body></html>", "https://a.test/x.zip").is_empty());
    }

    #[test]
    fn attribute_names_match_whole_words_only() {
        assert_eq!(
            attribute(r#"<meta data-name="x" name="og:title""#, "name").as_deref(),
            Some("og:title")
        );
    }

    #[test]
    fn non_web_images_are_dropped() {
        let html = r#"<meta property="og:image" content="data:image/png;base64,AAAA">"#;
        assert_eq!(parse_metadata(html, "https://a.test/").image_url, None);
    }

    #[test]
    fn hosts_drop_the_www_prefix() {
        assert_eq!(
            display_host("https://www.example.com/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            display_host("https://github.com/a").as_deref(),
            Some("github.com")
        );
        assert_eq!(display_host("not a url"), None);
    }
}
