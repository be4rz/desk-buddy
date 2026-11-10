//! What `buddy music play <query…>` plays: a YouTube URL or video id as given, otherwise the
//! first results of a YouTube search (the results page, fetched with `/usr/bin/curl` and an empty
//! environment; no API key). Debug builds' fake media switch resolves locally instead: `blocked…`
//! for a query naming `embed-blocked` (its player fails with error 150), else stable fake ids.

use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub id: String,
    pub title: String,
}

/// How many search results `next` can step through.
pub const MAX_RESULTS: usize = 8;

pub fn is_video_id(s: &str) -> bool {
    s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A YouTube watch / short / embed URL's video id.
pub fn id_of_url(s: &str) -> Option<String> {
    let s = s.trim();
    let rest = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.strip_prefix("www.").or_else(|| host.strip_prefix("m.")).or_else(|| host.strip_prefix("music.")).unwrap_or(host);
    let id = match host {
        "youtu.be" => path.split(['?', '/', '#']).next()?.to_string(),
        "youtube.com" | "youtube-nocookie.com" => {
            if let Some(q) = path.strip_prefix("watch?") {
                q.split('&').find_map(|kv| kv.strip_prefix("v=")).map(str::to_string)?
            } else {
                let mut it = path.split(['/', '?', '#']);
                match it.next() {
                    Some("embed" | "shorts" | "live" | "v") => it.next()?.to_string(),
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    is_video_id(&id).then_some(id)
}

/// The watch page of `id` (where an embed-blocked video opens).
pub fn watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// Video ids in a results page, in order, without repeats.
pub fn parse_results(html: &str) -> Vec<Video> {
    let mut out: Vec<Video> = vec![];
    let key = "\"videoId\":\"";
    let mut at = 0;
    while let Some(i) = html[at..].find(key) {
        let start = at + i + key.len();
        at = start;
        let Some(id) = html.get(start..start + 11) else { break };
        if !is_video_id(id) || html.as_bytes().get(start + 11) != Some(&b'"') || out.iter().any(|v| v.id == id) {
            continue;
        }
        // the renderer's title follows the id: "title":{"runs":[{"text":"…"}]
        let mut end = html.len().min(start + 4000);
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        let window = &html[start..end];
        let title = window.find("\"title\":{\"runs\":[{\"text\":\"").and_then(|t| {
            let s = &window[t + 26..];
            json_string_prefix(s)
        });
        out.push(Video { id: id.to_string(), title: title.unwrap_or_default() });
        if out.len() >= MAX_RESULTS {
            break;
        }
    }
    out
}

/// The JSON string body at the start of `s` (after its opening quote), unescaped.
fn json_string_prefix(s: &str) -> Option<String> {
    let mut end = None;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len().min(400) {
        match b[i] {
            b'\\' => i += 2,
            b'"' => {
                end = Some(i);
                break;
            }
            _ => i += 1,
        }
    }
    serde_json::from_str::<String>(&format!("\"{}\"", &s[..end?])).ok()
}

/// Debug builds: deterministic local ids.
pub fn fake(query: &str) -> Vec<Video> {
    if query.contains("embed-blocked") {
        return vec![Video { id: "blocked0150".into(), title: "A video whose owner blocks embedding".into() }];
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in query.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    (0..3).map(|n| Video { id: format!("fk{:09x}", (h.wrapping_add(n)) & 0xf_ffff_ffff), title: format!("{query} (test video {})", n + 1) }).collect()
}

/// Search YouTube for `query` (at most ~8 s).
pub fn search(query: &str) -> Result<Vec<Video>, String> {
    let out = Command::new("/usr/bin/curl")
        .env_clear()
        .args(["-sS", "-L", "--max-time", "8", "--proto", "=https", "-A", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "-H", "Accept-Language: en", "-G", "--data-urlencode"])
        .arg(format!("search_query={query}"))
        .arg("https://www.youtube.com/results")
        .output()
        .map_err(|e| format!("couldn't search YouTube ({e})"))?;
    if !out.status.success() {
        return Err(format!("couldn't search YouTube ({})", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let found = parse_results(&String::from_utf8_lossy(&out.stdout));
    if found.is_empty() {
        return Err(format!("YouTube found nothing for “{query}”"));
    }
    Ok(found)
}

/// `query` as videos to play: a URL or id as given, else `search` (or `fake`).
pub fn resolve(query: &str, use_fake: bool, search: &dyn Fn(&str) -> Result<Vec<Video>, String>) -> Result<Vec<Video>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("play what? buddy music play <query>".into());
    }
    if let Some(id) = id_of_url(q) {
        return Ok(vec![Video { id, title: String::new() }]);
    }
    // a bare id has a digit, `-` or `_`, or capitals past the first letter ("Tchaikovsky" is a query)
    if is_video_id(q) && (q.bytes().any(|b| b.is_ascii_digit() || b == b'-' || b == b'_') || q.bytes().skip(1).any(|b| b.is_ascii_uppercase())) {
        return Ok(vec![Video { id: q.into(), title: String::new() }]);
    }
    if use_fake {
        return Ok(fake(q));
    }
    search(q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_ids() {
        assert_eq!(id_of_url("https://www.youtube.com/watch?v=jfKfPfyJRdk&t=3").as_deref(), Some("jfKfPfyJRdk"));
        assert_eq!(id_of_url("https://youtu.be/jfKfPfyJRdk?si=x").as_deref(), Some("jfKfPfyJRdk"));
        assert_eq!(id_of_url("https://www.youtube.com/embed/jfKfPfyJRdk").as_deref(), Some("jfKfPfyJRdk"));
        assert_eq!(id_of_url("https://music.youtube.com/watch?list=x&v=jfKfPfyJRdk").as_deref(), Some("jfKfPfyJRdk"));
        assert_eq!(id_of_url("https://evil.example/watch?v=jfKfPfyJRdk"), None);
        assert_eq!(id_of_url("https://www.youtube.com/watch?v=short"), None);
        let none = |_: &str| -> Result<Vec<Video>, String> { panic!("no search for an id") };
        assert_eq!(resolve("jfKfPfyJRdk", false, &none).unwrap()[0].id, "jfKfPfyJRdk");
        // an 11-letter word is a search, not an id
        let s = |q: &str| Ok(vec![Video { id: "aaaaaaaaaaa".into(), title: q.into() }]);
        assert_eq!(resolve("synthesizer", false, &s).unwrap()[0].title, "synthesizer");
        assert_eq!(resolve("Tchaikovsky", false, &s).unwrap()[0].title, "Tchaikovsky");
        assert_eq!(resolve("Springsteen", false, &s).unwrap()[0].title, "Springsteen");
        assert_eq!(resolve("M7lc1UVf-VE", false, &none).unwrap()[0].id, "M7lc1UVf-VE");
        assert!(resolve("  ", false, &s).is_err());
    }

    #[test]
    fn a_results_page_full_of_wide_characters_never_splits_one() {
        // the 4000-byte title window would end inside a 3-byte character at every offset
        for pad in 0..3 {
            let html = format!(r#""videoId":"jfKfPfyJRdk"{}{}"#, "x".repeat(pad), "日".repeat(2000));
            assert_eq!(parse_results(&html)[0].id, "jfKfPfyJRdk");
        }
    }

    #[test]
    fn results_page_parsing() {
        let html = r#"..."videoId":"jfKfPfyJRdk","thumbnail":{},"title":{"runs":[{"text":"lofi hip hop radio & beats"}]}..."videoId":"jfKfPfyJRdk"..."videoId":"5qap5aO4i9A","title":{"runs":[{"text":"x"}]}..."videoId":"bad"#;
        let v = parse_results(html);
        assert_eq!(v.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["jfKfPfyJRdk", "5qap5aO4i9A"]);
        assert_eq!(v[0].title, "lofi hip hop radio & beats");
    }

    #[test]
    fn fake_ids() {
        let a = fake("lofi");
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|v| is_video_id(&v.id)), "{a:?}");
        assert_eq!(a, fake("lofi"));
        assert_eq!(fake("an embed-blocked video")[0].id, "blocked0150");
        assert!(is_video_id("blocked0150"));
    }
}
