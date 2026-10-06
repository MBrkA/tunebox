//! Thumbnail loading: an egui [`BytesLoader`] for `http(s)` URIs backed by an
//! in-memory map and a size-capped on-disk cache. Decoding is left to
//! `egui_extras`' image loader, which keeps decoded textures per URI.
//!
//! The same bytes feed [`dominant_color`], computed once per image on a
//! blocking thread and cached, for the now-playing backdrop.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
    Color32,
};

enum Entry {
    Pending,
    Ready(Arc<[u8]>),
    Failed(String),
}

pub struct ThumbLoader {
    rt: tokio::runtime::Handle,
    client: reqwest::Client,
    dir: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
    colors: Mutex<HashMap<String, Option<Color32>>>,
}

impl ThumbLoader {
    pub fn new(rt: tokio::runtime::Handle, dir: PathBuf, max_mb: u64) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&dir);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("http client");
        let loader = Arc::new(Self {
            rt: rt.clone(),
            client,
            dir: dir.clone(),
            entries: Mutex::default(),
            colors: Mutex::default(),
        });
        rt.spawn_blocking(move || prune_cache(&dir, max_mb * 1024 * 1024));
        loader
    }

    fn path_for(&self, uri: &str) -> PathBuf {
        let mut h = DefaultHasher::new();
        uri.hash(&mut h);
        self.dir.join(format!("{:016x}.img", h.finish()))
    }

    fn spawn_fetch(self: &Arc<Self>, ctx: &egui::Context, uri: &str) {
        let (this, ctx, uri) = (self.clone(), ctx.clone(), uri.to_owned());
        self.rt.spawn(async move {
            let path = this.path_for(&uri);
            let result = match tokio::fs::read(&path).await {
                Ok(bytes) if !bytes.is_empty() => Ok(bytes),
                _ => fetch(&this.client, &uri, &path).await,
            };
            let entry = match result {
                Ok(b) => Entry::Ready(b.into()),
                Err(e) => {
                    tracing::debug!(uri = %uri, error = %e, "thumbnail failed");
                    Entry::Failed(e)
                }
            };
            this.entries.lock().unwrap().insert(uri, entry);
            ctx.request_repaint();
        });
    }

    /// Dominant, text-safe colour of the image at `uri`, once available.
    /// Starts the (one-off, off-thread) computation on first call.
    pub fn dominant_color(self: &Arc<Self>, ctx: &egui::Context, uri: &str) -> Option<Color32> {
        {
            let colors = self.colors.lock().unwrap();
            if let Some(c) = colors.get(uri) {
                return *c;
            }
        }
        let bytes = match self.entries.lock().unwrap().get(uri) {
            Some(Entry::Ready(b)) => b.clone(),
            _ => return None,
        };
        self.colors.lock().unwrap().insert(uri.to_owned(), None);
        let (this, ctx, uri) = (self.clone(), ctx.clone(), uri.to_owned());
        self.rt.spawn_blocking(move || {
            let color = compute_dominant(&bytes);
            this.colors.lock().unwrap().insert(uri, color);
            ctx.request_repaint();
        });
        None
    }
}

/// A loader handle that can be registered with egui (needs `Arc<Self>` to spawn).
pub struct ThumbBytesLoader(pub Arc<ThumbLoader>);

impl BytesLoader for ThumbBytesLoader {
    fn id(&self) -> &str {
        "tunebox-thumbnails"
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("http://") || uri.starts_with("https://")) {
            return Err(LoadError::NotSupported);
        }
        let mut entries = self.0.entries.lock().unwrap();
        match entries.get(uri) {
            Some(Entry::Ready(b)) => Ok(BytesPoll::Ready {
                size: None,
                bytes: Bytes::Shared(b.clone()),
                mime: None,
            }),
            Some(Entry::Pending) => Ok(BytesPoll::Pending { size: None }),
            Some(Entry::Failed(e)) => Err(LoadError::Loading(e.clone())),
            None => {
                entries.insert(uri.to_owned(), Entry::Pending);
                drop(entries);
                self.0.spawn_fetch(ctx, uri);
                Ok(BytesPoll::Pending { size: None })
            }
        }
    }

    fn forget(&self, uri: &str) {
        self.0.entries.lock().unwrap().remove(uri);
    }

    fn forget_all(&self) {
        self.0.entries.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.0
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|e| match e {
                Entry::Ready(b) => b.len(),
                _ => 0,
            })
            .sum()
    }
}

async fn fetch(client: &reqwest::Client, uri: &str, path: &Path) -> Result<Vec<u8>, String> {
    let resp = client
        .get(uri)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
    // Write via a temp file so a crash never leaves a truncated image behind.
    let tmp = path.with_extension("part");
    if tokio::fs::write(&tmp, &bytes).await.is_ok() {
        let _ = tokio::fs::rename(&tmp, path).await;
    }
    Ok(bytes)
}

/// Deletes the oldest cached files until the directory is under `max_bytes`.
pub fn prune_cache(dir: &Path, max_bytes: u64) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = read
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= max_bytes {
        return;
    }
    files.sort_by_key(|f| f.2);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// Rewrites a googleusercontent thumbnail URL (`…=w120-h120-l90-rj`) to `px`×`px`.
/// Other URLs (e.g. `i.ytimg.com`) are returned unchanged.
pub fn sized_url(url: &str, px: u32) -> String {
    let Some(eq) = url.rfind('=') else {
        return url.to_owned();
    };
    let (head, tail) = url.split_at(eq + 1);
    let mut parts = tail.split('-');
    match (parts.next(), parts.next()) {
        (Some(w), Some(h))
            if w.starts_with('w')
                && h.starts_with('h')
                && w[1..].bytes().all(|b| b.is_ascii_digit())
                && h[1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            let rest: Vec<&str> = parts.collect();
            let mut out = format!("{head}w{px}-h{px}");
            for r in rest {
                out.push('-');
                out.push_str(r);
            }
            out
        }
        _ => url.to_owned(),
    }
}

/// Picks a vivid colour that represents the image, darkened so white text stays readable on it.
pub fn compute_dominant(bytes: &[u8]) -> Option<Color32> {
    let img = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(48, 48)
        .to_rgb8();
    const BINS: usize = 12;
    let mut weight = [0.0f32; BINS];
    let mut sum = [[0.0f32; 3]; BINS];
    for p in img.pixels() {
        let (r, g, b) = (
            f32::from(p[0]) / 255.0,
            f32::from(p[1]) / 255.0,
            f32::from(p[2]) / 255.0,
        );
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if max < 0.12 {
            continue; // near-black pixels carry no colour information
        }
        let hue = if max == min {
            0.0
        } else if max == r {
            ((g - b) / (max - min)).rem_euclid(6.0)
        } else if max == g {
            (b - r) / (max - min) + 2.0
        } else {
            (r - g) / (max - min) + 4.0
        } / 6.0;
        let bin = ((hue * BINS as f32) as usize).min(BINS - 1);
        let w = sat * sat * max + 0.002; // favour saturated, bright pixels
        weight[bin] += w;
        for (acc, v) in sum[bin].iter_mut().zip([r, g, b]) {
            *acc += v * w;
        }
    }
    let (best, total) = weight
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, w)| (i, *w))?;
    if total <= 0.0 {
        return None;
    }
    let [r, g, b] = sum[best].map(|v| v / total);
    // Cap brightness so the gradient never fights the white UI text.
    let max = r.max(g).max(b).max(1e-3);
    let scale = (0.55 / max).min(1.0);
    let to8 = |v: f32| (v * scale * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Color32::from_rgb(to8(r), to8(g), to8(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizes_googleusercontent_urls() {
        assert_eq!(
            sized_url("https://yt3.googleusercontent.com/abc=w60-h60-l90-rj", 544),
            "https://yt3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
        assert_eq!(
            sized_url("https://lh3.googleusercontent.com/x=w120-h120", 226),
            "https://lh3.googleusercontent.com/x=w226-h226"
        );
        let ytimg = "https://i.ytimg.com/vi/abc/sddefault.jpg?sqp=x=y";
        assert_eq!(sized_url(ytimg, 300), ytimg);
        assert_eq!(sized_url("https://x/y", 300), "https://x/y");
    }

    fn png(color: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(32, 32, image::Rgb(color));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn dominant_color_is_dark_enough_for_white_text() {
        let c = compute_dominant(&png([250, 40, 40])).unwrap();
        assert!(c.r() > c.g() && c.r() > c.b(), "{c:?}");
        assert!(c.r() <= 142, "brightness capped: {c:?}");
        // fully black images give no colour
        assert_eq!(compute_dominant(&png([0, 0, 0])), None);
        assert_eq!(compute_dominant(b"not an image"), None);
    }

    #[test]
    fn dominant_color_prefers_saturated_over_grey() {
        let mut img = image::RgbImage::from_pixel(32, 32, image::Rgb([128, 128, 128]));
        for x in 0..8 {
            for y in 0..8 {
                img.put_pixel(x, y, image::Rgb([20, 60, 230]));
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let c = compute_dominant(&out.into_inner()).unwrap();
        assert!(c.b() > c.r() && c.b() > c.g(), "{c:?}");
    }

    #[test]
    fn prune_removes_oldest_first() {
        let dir = tempfile_dir();
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let p = dir.join(name);
            std::fs::write(&p, vec![0u8; 100]).unwrap();
            let t = std::time::SystemTime::now() - Duration::from_secs(100 - i as u64 * 10);
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        prune_cache(&dir, 150);
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["c"], "oldest two removed");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tunebox-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
