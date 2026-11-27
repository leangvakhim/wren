//! More folder icons, kept on this computer (the 2,124 Lucide icons ship inside
//! the app, see scripts/gen-icons.mjs):
//! - the Tabler pack, downloaded only when you click Download, checked against
//!   a fixed SHA-256 and reduced to plain shapes (no scripts, styles, links or
//!   other markup can reach Wren's page), saved in `<app_data>/icons/tabler.json`;
//! - pictures you add yourself, copied into `<app_data>/icons/own` and shown
//!   only through <img> (an SVG there can't run scripts or load anything).

use crate::attachments::sniff_image;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TABLER_VERSION: &str = "3.53.0";
/// (file in the npm package, its SHA-256). Same bytes on jsDelivr and npm.
const TABLER_FILES: [(&str, &str); 2] = [
    ("tabler-nodes-outline.json", "9effe2e088c26e81eed18304a1cc1203008dbb084a9ba3d77c1a5c231fa21ff7"),
    ("icons.json", "5389f6f3ef81aaa6baace06c144e170ed4669c737715bb64b97b6f4b14d2d4ca"),
];
const MAX_DOWNLOAD: usize = 8 << 20;
/// Your own pictures: at most this big, and this many.
pub const MAX_OWN_BYTES: u64 = 1 << 20;
const MAX_OWN: usize = 300;

/// One icon of a pack: SVG markup made only of plain shapes.
#[derive(Serialize, Clone, Debug)]
pub struct PackIcon {
    pub id: String,
    pub category: String,
    pub tags: String,
    pub body: String,
}

#[derive(Serialize, Debug)]
pub struct Pack {
    pub version: String,
    pub icons: Vec<PackIcon>,
}

#[derive(Serialize, Debug)]
pub struct OwnIcon {
    pub id: String,
    /// data: URL for an <img>.
    pub url: String,
}

/// The saved pack keeps the shapes (checked again when loaded), not markup.
#[derive(Serialize, Deserialize)]
struct Saved {
    version: String,
    icons: Vec<SavedIcon>,
}

#[derive(Serialize, Deserialize)]
struct SavedIcon {
    id: String,
    category: String,
    tags: String,
    shapes: Vec<Shape>,
}

type Shape = (String, Vec<(String, String)>);

fn dir(app_data: &Path) -> PathBuf {
    app_data.join("icons")
}

fn private_dir(d: &Path) -> Result<(), String> {
    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

// ---- Shapes -> markup (the same rules as scripts/gen-icons.mjs) ----

const SHAPES: [&str; 7] = ["path", "circle", "rect", "line", "polyline", "polygon", "ellipse"];

fn value_ok(attr: &str, v: &str) -> bool {
    let geometry = |v: &str| v.len() <= 4000 && v.chars().all(|c| c.is_ascii_digit() || "MmLlHhVvCcSsQqTtAaZzEe.,+- ".contains(c));
    let number = |v: &str| v.len() <= 32 && v.chars().all(|c| c.is_ascii_digit() || ".eE+-".contains(c));
    !v.is_empty()
        && match attr {
            "d" | "points" => geometry(v),
            "cx" | "cy" | "r" | "rx" | "ry" | "x" | "y" | "width" | "height" | "x1" | "y1" | "x2" | "y2" | "opacity" => number(v),
            "fill" | "stroke" => v == "none" || v == "currentColor",
            _ => false,
        }
}

/// SVG markup for the shapes, or None if anything isn't a plain shape.
fn markup(shapes: &[Shape]) -> Option<String> {
    let mut s = String::new();
    for (tag, attrs) in shapes {
        if !SHAPES.contains(&tag.as_str()) {
            return None;
        }
        s.push('<');
        s.push_str(tag);
        for (k, v) in attrs {
            if !value_ok(k, v) {
                return None;
            }
            s.push_str(&format!(" {k}=\"{v}\""));
        }
        s.push_str("/>");
    }
    (!s.is_empty() && s.len() <= 8000).then_some(s)
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Tabler's two files -> the shapes Wren keeps. Icons with anything unusual are left out.
fn parse_tabler(nodes: &[u8], meta: &[u8]) -> Result<Saved, String> {
    let bad = |e: serde_json::Error| format!("The icon files couldn't be read ({e}).");
    let nodes: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(nodes).map_err(bad)?;
    let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(meta).map_err(bad)?;
    let text = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let mut icons = Vec::new();
    for (id, list) in &nodes {
        let Some(list) = list.as_array() else { continue };
        let shapes: Option<Vec<Shape>> = list
            .iter()
            .map(|n| {
                let tag = n.get(0)?.as_str()?.to_string();
                let attrs = n.get(1)?.as_object()?.iter().map(|(k, v)| Some((k.clone(), text(v)?))).collect::<Option<Vec<_>>>()?;
                Some((tag, attrs))
            })
            .collect();
        let Some(shapes) = shapes.filter(|s| id_ok(id) && markup(s).is_some()) else { continue };
        let m = meta.get(id);
        let category = m.and_then(|m| m["category"].as_str()).unwrap_or("").chars().take(40).collect();
        let tags = m
            .and_then(|m| m["tags"].as_array())
            .map(|t| t.iter().filter_map(text).collect::<Vec<_>>().join(","))
            .unwrap_or_default()
            .chars()
            .take(600)
            .collect();
        icons.push(SavedIcon { id: id.clone(), category, tags, shapes });
    }
    if icons.is_empty() {
        return Err("The downloaded file had no icons.".into());
    }
    Ok(Saved { version: TABLER_VERSION.into(), icons })
}

fn to_pack(saved: Saved) -> Pack {
    let icons = saved
        .icons
        .into_iter()
        .filter(|i| id_ok(&i.id))
        .filter_map(|i| Some(PackIcon { body: markup(&i.shapes)?, id: i.id, category: i.category, tags: i.tags }))
        .collect();
    Pack { version: saved.version, icons }
}

// ---- The Tabler pack ----

/// The downloaded pack, if any.
pub fn load_pack(app_data: &Path) -> Option<Pack> {
    let text = std::fs::read(dir(app_data).join("tabler.json")).ok()?;
    serde_json::from_slice::<Saved>(&text).ok().map(to_pack)
}

pub fn remove_pack(app_data: &Path) -> Result<(), String> {
    match std::fs::remove_file(dir(app_data).join("tabler.json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn fetch(client: &reqwest::Client, name: &str, sha: &str) -> Result<Vec<u8>, String> {
    let url = format!("https://cdn.jsdelivr.net/npm/@tabler/icons@{TABLER_VERSION}/{name}");
    let fail = |e: String| format!("Couldn't download the icons ({e}). Check the internet connection and try again.");
    let resp = client.get(&url).send().await.map_err(|e| fail(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(fail(format!("the server answered {}", resp.status())));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_DOWNLOAD) {
        return Err("The icon file is bigger than expected, so Wren didn't download it.".into());
    }
    let bytes = resp.bytes().await.map_err(|e| fail(e.to_string()))?;
    if bytes.len() > MAX_DOWNLOAD || hex(&Sha256::digest(&bytes)) != sha {
        return Err("The downloaded icons weren't the expected files, so Wren didn't use them.".into());
    }
    Ok(bytes.to_vec())
}

/// Downloads Tabler's icons from jsDelivr, checks them and saves the shapes.
pub async fn download_pack(app_data: &Path) -> Result<Pack, String> {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().timeout(Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
    let nodes = fetch(&client, TABLER_FILES[0].0, TABLER_FILES[0].1).await?;
    let meta = fetch(&client, TABLER_FILES[1].0, TABLER_FILES[1].1).await?;
    let saved = parse_tabler(&nodes, &meta)?;
    let d = dir(app_data);
    private_dir(&d)?;
    let tmp = d.join("tabler.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&saved).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, d.join("tabler.json")).map_err(|e| e.to_string())?;
    Ok(to_pack(saved))
}

// ---- Your own pictures ----

fn own_dir(app_data: &Path) -> PathBuf {
    dir(app_data).join("own")
}

/// "image/svg+xml" if the text looks like an SVG file.
fn sniff_svg(bytes: &[u8]) -> Option<&'static str> {
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).ok()?;
    let start = text.trim_start();
    ((start.starts_with("<?xml") || start.starts_with("<svg") || start.starts_with("<!--") || start.starts_with("<!DOCTYPE svg"))
        && text.contains("<svg"))
    .then_some("image/svg+xml")
}

fn ext_of(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "svg",
    }
}

fn mime_of_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Your pictures, oldest first.
pub fn list_own(app_data: &Path) -> Vec<OwnIcon> {
    let Ok(rd) = std::fs::read_dir(own_dir(app_data)) else { return vec![] };
    let mut found: Vec<(std::time::SystemTime, OwnIcon)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (id, ext) = name.split_once('.')?;
            let mime = mime_of_ext(ext).filter(|_| own_id_ok(id))?;
            let meta = e.metadata().ok().filter(|m| m.is_file() && m.len() <= MAX_OWN_BYTES)?;
            let bytes = std::fs::read(e.path()).ok()?;
            Some((meta.modified().unwrap_or(std::time::UNIX_EPOCH), OwnIcon { id: id.into(), url: data_url(mime, &bytes) }))
        })
        .collect();
    found.sort_by_key(|(t, _)| *t);
    found.into_iter().map(|(_, i)| i).collect()
}

fn own_id_ok(id: &str) -> bool {
    id.len() == 16 && id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Copies a picture (SVG, PNG, JPEG, GIF or WebP, up to 1 MB) into Wren's folder.
pub fn add_own(app_data: &Path, src: &Path) -> Result<OwnIcon, String> {
    let meta = std::fs::metadata(src).map_err(|e| format!("Couldn't read the file ({e})."))?;
    if !meta.is_file() || meta.len() == 0 {
        return Err("That isn't a picture file.".into());
    }
    if meta.len() > MAX_OWN_BYTES {
        return Err("Pictures for icons can be up to 1 MB.".into());
    }
    let bytes = std::fs::read(src).map_err(|e| format!("Couldn't read the file ({e})."))?;
    let mime = sniff_image(&bytes)
        .or_else(|| sniff_svg(&bytes))
        .ok_or("Only SVG, PNG, JPEG, GIF and WebP pictures can be icons.")?;
    let d = own_dir(app_data);
    private_dir(&d)?;
    if std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0) >= MAX_OWN {
        return Err(format!("You can keep up to {MAX_OWN} pictures. Remove some first."));
    }
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    let id = hex(&raw);
    std::fs::write(d.join(format!("{id}.{}", ext_of(mime))), &bytes).map_err(|e| format!("Couldn't save the picture ({e})."))?;
    Ok(OwnIcon { url: data_url(mime, &bytes), id })
}

pub fn remove_own(app_data: &Path, id: &str) -> Result<(), String> {
    if !own_id_ok(id) {
        return Err("Unknown picture.".into());
    }
    for ext in ["png", "jpg", "gif", "webp", "svg"] {
        let _ = std::fs::remove_file(own_dir(app_data).join(format!("{id}.{ext}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wren-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_plain_shapes_pass() {
        let ok = |t: &str, a: &[(&str, &str)]| vec![(t.to_string(), a.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())];
        assert_eq!(markup(&ok("path", &[("d", "M3 7a2 2 0 0 1 2-2h4")])).unwrap(), r#"<path d="M3 7a2 2 0 0 1 2-2h4"/>"#);
        assert!(markup(&ok("circle", &[("cx", "12"), ("cy", "12"), ("r", "1.5"), ("fill", "currentColor")])).is_some());
        // Anything that could carry markup, scripts, links or styles is refused.
        assert!(markup(&ok("script", &[])).is_none());
        assert!(markup(&ok("image", &[("x", "0")])).is_none());
        assert!(markup(&ok("path", &[("d", "M0 0\"/><script>")])).is_none());
        assert!(markup(&ok("path", &[("onload", "alert(1)")])).is_none());
        assert!(markup(&ok("path", &[("style", "x")])).is_none());
        assert!(markup(&ok("path", &[("fill", "url(#x)")])).is_none());
        assert!(markup(&ok("rect", &[("width", "javascript:1")])).is_none());
        assert!(markup(&[]).is_none());
    }

    #[test]
    fn tabler_files_become_a_pack() {
        let nodes = br#"{"a-b":[["path",{"d":"M3 3h18"}],["circle",{"cx":"12","cy":"12","r":2}]],
            "evil":[["path",{"d":"M0 0","onclick":"x"}]], "Bad Name":[["path",{"d":"M0 0"}]], "foreign":[["foreignObject",{}]]}"#;
        let meta = br#"{"a-b":{"category":"Text","tags":["ab",1]}}"#;
        let pack = to_pack(parse_tabler(nodes, meta).unwrap());
        assert_eq!(pack.icons.len(), 1);
        let i = &pack.icons[0];
        assert_eq!((i.id.as_str(), i.category.as_str(), i.tags.as_str()), ("a-b", "Text", "ab,1"));
        assert_eq!(i.body, r#"<path d="M3 3h18"/><circle cx="12" cy="12" r="2"/>"#);
        assert!(parse_tabler(br#"{"x":[["script",{}]]}"#, b"{}").is_err());
    }

    #[test]
    fn saved_pack_is_checked_again_when_loaded() {
        let d = tmp("pack");
        let saved = Saved {
            version: "1".into(),
            icons: vec![
                SavedIcon { id: "ok".into(), category: String::new(), tags: String::new(), shapes: vec![("path".into(), vec![("d".into(), "M1 1".into())])] },
                SavedIcon { id: "bad".into(), category: String::new(), tags: String::new(), shapes: vec![("script".into(), vec![])] },
            ],
        };
        std::fs::create_dir_all(dir(&d)).unwrap();
        std::fs::write(dir(&d).join("tabler.json"), serde_json::to_vec(&saved).unwrap()).unwrap();
        let pack = load_pack(&d).unwrap();
        assert_eq!(pack.icons.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["ok"]);
        remove_pack(&d).unwrap();
        assert!(load_pack(&d).is_none());
        remove_pack(&d).unwrap();
    }

    #[test]
    fn own_pictures_are_copied_checked_and_removed() {
        let d = tmp("own");
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let png = src.join("logo.png");
        std::fs::write(&png, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]).unwrap();
        let svg = src.join("logo.svg");
        std::fs::write(&svg, "\u{feff}<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
        let exe = src.join("app.png");
        std::fs::write(&exe, b"MZ\x90\x00").unwrap();
        let big = src.join("big.png");
        std::fs::write(&big, vec![0u8; MAX_OWN_BYTES as usize + 1]).unwrap();

        let a = add_own(&d, &png).unwrap();
        assert!(a.url.starts_with("data:image/png;base64,"));
        let b = add_own(&d, &svg).unwrap();
        assert!(b.url.starts_with("data:image/svg+xml;base64,"));
        assert!(add_own(&d, &exe).is_err());
        assert!(add_own(&d, &big).is_err());
        assert!(add_own(&d, &src).is_err());
        assert_eq!(list_own(&d).len(), 2);
        assert!(remove_own(&d, "../../etc").is_err());
        remove_own(&d, &a.id).unwrap();
        assert_eq!(list_own(&d).iter().map(|i| i.id.clone()).collect::<Vec<_>>(), [b.id]);
    }

    /// WREN_REAL_DOWNLOAD=1 cargo test real_tabler_download -- --nocapture
    #[tokio::test]
    async fn real_tabler_download() {
        if std::env::var("WREN_REAL_DOWNLOAD").is_err() {
            return;
        }
        let d = tmp("real");
        let pack = download_pack(&d).await.unwrap();
        println!("{} icons", pack.icons.len());
        assert!(pack.icons.len() > 5000);
        assert_eq!(load_pack(&d).unwrap().icons.len(), pack.icons.len());
    }
}
