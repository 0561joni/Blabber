//! A tiny local web server for verifying references (`corpus review`).
//! Listens on 127.0.0.1 only; audio is served by clip id, never by path.
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::corpus::{self, Manifest, Scripts};
use crate::paths::Paths;

const REVIEW_HTML: &str = include_str!("review.html");
const MAX_BODY: usize = 4 * 1024 * 1024;

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().context("empty request")?.to_string();
    let path = parts.next().context("no path")?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
        if headers.len() > 100 {
            anyhow::bail!("too many headers");
        }
    }
    let length: usize = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    anyhow::ensure!(length <= MAX_BODY, "request body too large");
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request { method, path, headers, body })
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, extra: &[(&str, String)], body: &[u8]) -> Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (key, value) in extra {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

fn respond_json(stream: &mut TcpStream, status: &str, value: serde_json::Value) -> Result<()> {
    respond(stream, status, "application/json; charset=utf-8", &[], serde_json::to_string(&value)?.as_bytes())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn clips_json(paths: &Paths, scripts: &Scripts) -> Result<serde_json::Value> {
    let manifest = Manifest::load(paths)?;
    let clips: Vec<serde_json::Value> = manifest
        .clips
        .iter()
        .map(|clip| {
            json!({
                "id": clip.id,
                "language": clip.language,
                "languages": clip.languages,
                "category": clip.category,
                "tags": clip.tags,
                "condition": clip.condition,
                "script": clip.script,
                "durationMs": clip.duration_ms,
                "reference": clip.reference,
                "referenceStatus": clip.reference_status,
                "scriptText": clip.script.as_deref().and_then(|id| scripts.text(id)),
                "terms": clip.terms,
                "notes": clip.notes,
                "drafts": corpus::load_drafts(paths, &clip.id),
            })
        })
        .collect();
    Ok(json!({"conventions": corpus::CONVENTIONS, "clips": clips}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Update {
    reference: String,
    reference_status: String,
    #[serde(default)]
    terms: Vec<String>,
    #[serde(default)]
    notes: String,
}

fn save(paths: &Paths, lock: &Mutex<()>, id: &str, body: &[u8]) -> Result<Result<(), (&'static str, String)>> {
    let update: Update = match serde_json::from_slice(body) {
        Ok(update) => update,
        Err(error) => return Ok(Err(("400 Bad Request", format!("invalid body: {error}")))),
    };
    if !["draft", "verified"].contains(&update.reference_status.as_str()) {
        return Ok(Err(("400 Bad Request", "referenceStatus must be draft or verified".into())));
    }
    let _guard = lock.lock().map_err(|_| anyhow::anyhow!("manifest lock poisoned"))?;
    let mut manifest = Manifest::load(paths)?;
    let Some(clip) = manifest.get_mut(id) else {
        return Ok(Err(("404 Not Found", format!("no clip {id}"))));
    };
    if update.reference_status == "verified" && clip.language != "none" && update.reference.trim().is_empty() {
        return Ok(Err(("400 Bad Request", "a speech clip cannot be verified with an empty reference".into())));
    }
    clip.reference = update.reference.trim().to_string();
    clip.reference_status = update.reference_status;
    clip.terms = update.terms.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    clip.notes = update.notes;
    manifest.save(paths)?;
    Ok(Ok(()))
}

fn serve_audio(stream: &mut TcpStream, paths: &Paths, id: &str, range: Option<&str>) -> Result<()> {
    let manifest = Manifest::load(paths)?;
    let Some(clip) = manifest.clips.iter().find(|clip| clip.id == id) else {
        return respond(stream, "404 Not Found", "text/plain", &[], b"unknown clip");
    };
    let mut file = std::fs::File::open(clip.audio_path(&paths.corpus))?;
    let size = file.metadata()?.len();
    let parsed = range.and_then(|value| {
        let spec = value.strip_prefix("bytes=")?;
        let (start, end) = spec.split_once('-')?;
        let start: u64 = start.parse().ok()?;
        let end: u64 = if end.is_empty() { size.saturating_sub(1) } else { end.parse().ok()? };
        (start <= end && end < size).then_some((start, end))
    });
    match parsed {
        Some((start, end)) => {
            let mut body = vec![0; (end - start + 1) as usize];
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut body)?;
            respond(
                stream,
                "206 Partial Content",
                "audio/wav",
                &[("Accept-Ranges", "bytes".into()), ("Content-Range", format!("bytes {start}-{end}/{size}"))],
                &body,
            )
        }
        None if range.is_some() => respond(
            stream,
            "416 Range Not Satisfiable",
            "text/plain",
            &[("Content-Range", format!("bytes */{size}"))],
            b"",
        ),
        None => {
            let mut body = Vec::with_capacity(size as usize);
            file.read_to_end(&mut body)?;
            respond(stream, "200 OK", "audio/wav", &[("Accept-Ranges", "bytes".into())], &body)
        }
    }
}

fn handle(mut stream: TcpStream, paths: &Paths, scripts: &Scripts, lock: &Mutex<()>, port: u16) -> Result<()> {
    let request = read_request(&stream)?;
    // Refuse requests from other sites (DNS rebinding, cross-site POSTs).
    let host_ok = request
        .header("host")
        .is_some_and(|host| host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"));
    let origin_ok = request
        .header("origin")
        .is_none_or(|origin| origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}"));
    if !host_ok || !origin_ok {
        return respond(&mut stream, "403 Forbidden", "text/plain", &[], b"forbidden");
    }
    let path = request.path.split('?').next().unwrap_or("").to_string();
    match (request.method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => respond(&mut stream, "200 OK", "text/html; charset=utf-8", &[], REVIEW_HTML.as_bytes()),
        ("GET", "/api/clips") => {
            let value = clips_json(paths, scripts)?;
            respond_json(&mut stream, "200 OK", value)
        }
        ("GET", audio) if audio.starts_with("/audio/") => {
            let id = percent_decode(&audio["/audio/".len()..]);
            let range = request.header("range").map(str::to_string);
            serve_audio(&mut stream, paths, &id, range.as_deref())
        }
        ("POST", clip) if clip.starts_with("/api/clips/") => {
            let id = percent_decode(&clip["/api/clips/".len()..]);
            match save(paths, lock, &id, &request.body)? {
                Ok(()) => respond_json(&mut stream, "200 OK", json!({"ok": true})),
                Err((status, error)) => respond_json(&mut stream, status, json!({"ok": false, "error": error})),
            }
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", &[], b"not found"),
    }
}

pub fn serve(paths: &Paths, port: u16, open: bool) -> Result<()> {
    let manifest = Manifest::load(paths)?;
    if manifest.clips.is_empty() {
        anyhow::bail!("The corpus is empty. Import recordings first (`npm run bench -- corpus import <folder>`).");
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("port {port} is in use"))?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}/");
    let unverified = manifest.clips.iter().filter(|c| !c.verified()).count();
    println!("Review page: {url}  ({unverified} of {} clips unverified)", manifest.clips.len());
    println!("Edits are saved to {}. Press Ctrl+C to stop.", paths.manifest().display());
    if open {
        let _ = std::process::Command::new("open").arg(&url).status();
    }
    let paths = Arc::new(paths.clone());
    let scripts = Arc::new(Scripts::load()?);
    let lock = Arc::new(Mutex::new(()));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (paths, scripts, lock) = (paths.clone(), scripts.clone(), lock.clone());
        std::thread::spawn(move || {
            if let Err(error) = handle(stream, &paths, &scripts, &lock, port) {
                eprintln!("review server: {error:#}");
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn percent_decoding_handles_encoded_and_plain_ids() {
        assert_eq!(percent_decode("de-s05%40noisy"), "de-s05@noisy");
        assert_eq!(percent_decode("de-s05"), "de-s05");
        assert_eq!(percent_decode("bad%4"), "bad%4");
    }
}
