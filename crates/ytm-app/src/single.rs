//! Single instance: the first Tunebox listens on a loopback port; a later launch asks it to show
//! its window and exits (e.g. opening from the app menu while it sits in the tray).
//! Set `TUNEBOX_MULTI=1` to skip this (dev screenshots next to a running app).

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const ADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 47613);
const HELLO: &str = "tunebox-show";
const REPLY: &str = "ok";

static CTX: OnceLock<eframe::egui::Context> = OnceLock::new();
static SHOW: AtomicBool = AtomicBool::new(false);

pub enum Instance {
    /// This process is the (only) instance.
    Primary,
    /// Another Tunebox answered and was asked to show itself.
    Existing,
}

/// Becomes the primary instance, or asks the running one to come forward.
pub fn acquire() -> Instance {
    if std::env::var_os("TUNEBOX_MULTI").is_some() {
        return Instance::Primary;
    }
    match TcpListener::bind(ADDR) {
        Ok(listener) => {
            let _ = std::thread::Builder::new()
                .name("tunebox-single".into())
                .spawn(move || serve(listener));
            Instance::Primary
        }
        Err(_) if ask_running() => Instance::Existing,
        // The port is taken by something else (or unreachable): just run normally.
        Err(_) => Instance::Primary,
    }
}

fn ask_running() -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&ADDR, Duration::from_millis(500)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    if writeln!(s, "{HELLO}").is_err() {
        return false;
    }
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).is_ok() && line.trim() == REPLY
}

fn serve(listener: TcpListener) {
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_ok() && line.trim() == HELLO {
            let _ = writeln!(stream, "{REPLY}");
            SHOW.store(true, Ordering::SeqCst);
            if let Some(ctx) = CTX.get() {
                use eframe::egui::ViewportCommand as V;
                ctx.send_viewport_cmd(V::Visible(true));
                ctx.send_viewport_cmd(V::Minimized(false));
                ctx.send_viewport_cmd(V::Focus);
                ctx.request_repaint();
            }
        }
    }
}

/// Lets the listener wake the UI (call once the window exists).
pub fn set_context(ctx: eframe::egui::Context) {
    let _ = CTX.set(ctx);
}

/// True once per request from a second launch.
pub fn take_show_request() -> bool {
    SHOW.swap(false, Ordering::SeqCst)
}
