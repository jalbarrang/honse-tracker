//! Browse the Independent Training careers the plugin exports.
//!
//! ```text
//! cargo run -p career-viewer
//! → http://127.0.0.1:4173
//! ```
//!
//! # Why this is a crate and not the Node app it replaces
//!
//! The rank ladder, the stat-rank sprites, the career calendar and the
//! condition names all already existed in Rust, tested by this workspace's CI.
//! A viewer in another language meant a second hand-written copy of those
//! tables that nothing checked and that would drift the first time a condition
//! id was added. They live in `honse-career-meta` now and both readers share
//! them, which is the actual reason this moved — not tidiness about languages.
//!
//! It is deliberately outside `default-members`, so `cargo build --release` —
//! what the deploy script runs — never compiles a web stack to ship a plugin.
//!
//! # Configuration
//!
//! - `CAREERS_DIR` — where the plugin writes exports
//! - `HAKURAKU_ASSETS` — hakuraku's `public/assets`, served read-only at `/assets`
//! - `PORT`

mod assets;
mod career;
mod umdb;
mod view;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

use assets::Assets;

struct App {
    careers_dir: PathBuf,
    assets: Assets,
    umdb: umdb::Umdb,
}

#[tokio::main]
async fn main() {
    let careers_dir = env_path("CAREERS_DIR").unwrap_or_else(default_careers_dir);
    let assets_root = env_path("HAKURAKU_ASSETS").unwrap_or_else(|| hakuraku().join("public").join("assets"));
    let umdb_path = env_path("UMDB_JSON").unwrap_or_else(|| hakuraku().join("public").join("data").join("umdb.json"));
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4173);

    // Say what is missing at startup rather than rendering an empty page and
    // leaving someone to guess which of the two paths is wrong.
    if !careers_dir.is_dir() {
        eprintln!(
            "note: no careers directory at {} (set CAREERS_DIR)",
            careers_dir.display()
        );
    }
    if !assets_root.is_dir() {
        eprintln!(
            "note: no assets at {} (set HAKURAKU_ASSETS); pages will render without art",
            assets_root.display()
        );
    }

    // Names are a convenience, so a missing database degrades to raw ids
    // rather than refusing to start over someone else's checkout.
    let umdb = umdb::Umdb::load(&umdb_path).unwrap_or_else(|| {
        eprintln!(
            "note: no umdb at {} (set UMDB_JSON); ids will show unresolved",
            umdb_path.display()
        );
        umdb::Umdb::empty()
    });

    let app = Arc::new(App {
        careers_dir,
        assets: Assets::new(assets_root),
        umdb,
    });

    let router = Router::new()
        .route("/", get(index))
        .route("/career/{file}", get(detail))
        .route("/raw/{file}", get(raw))
        .nest_service(Assets::MOUNT, ServeDir::new(app.assets.root()))
        .with_state(Arc::clone(&app));

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("career-viewer → http://{addr}");
    println!("  careers: {}", app.careers_dir.display());
    println!("  assets:  {}", app.assets.root().display());
    if !app.umdb.is_empty() {
        println!("  names:   {}", umdb_path.display());
    }
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("server stopped: {e}");
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Where the plugin writes exports by default — the same rule it uses, from
/// the shared crate, so the two cannot disagree.
fn default_careers_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map_or_else(
        || PathBuf::from("SavedIdleCareers"),
        |home| honse_career_meta::saved_careers_dir(&PathBuf::from(home)),
    )
}

/// A hakuraku checkout beside this one: `cargo run` sets the working directory
/// to the workspace root, so its parent is where sibling repos live. Anyone
/// with a different layout sets `HAKURAKU_ASSETS` / `UMDB_JSON`.
fn hakuraku() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default()
        .join("hakuraku")
}

async fn index(State(app): State<Arc<App>>) -> Response {
    let entries = career::list(&app.careers_dir, &app.umdb);
    Html(view::index(&entries, &app.assets, &app.careers_dir).into_string()).into_response()
}

async fn detail(State(app): State<Arc<App>>, UrlPath(file): UrlPath<String>) -> Response {
    let Some(value) = load(&app, &file) else {
        return not_found(&file);
    };
    let parsed = career::parse(&file, &value, &app.umdb);
    Html(view::career(&parsed, &app.assets).into_string()).into_response()
}

async fn raw(State(app): State<Arc<App>>, UrlPath(file): UrlPath<String>) -> Response {
    let Some(value) = load(&app, &file) else {
        return not_found(&file);
    };
    match serde_json::to_string_pretty(&value) {
        Ok(json) => ([(axum::http::header::CONTENT_TYPE, "application/json")], json).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Resolve and read, or `None` — the two failures a caller can do nothing
/// different about, so they collapse into one answer.
fn load(app: &App, file: &str) -> Option<serde_json::Value> {
    career::read_json(&career::resolve(&app.careers_dir, file)?)
}

/// One answer for "refused" and "absent" alike.
///
/// Telling them apart would confirm which names exist to anyone poking at the
/// URL, and the honest answer to both is the same: there is no such career.
fn not_found(file: &str) -> Response {
    let body = view::message("No such career", &format!("Nothing readable at {file}.")).into_string();
    (StatusCode::NOT_FOUND, Html(body)).into_response()
}
