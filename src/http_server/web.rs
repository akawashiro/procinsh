//! Embedded list, process details, and SPACE pages and their static assets.
//!
//! # Interface
//! - [`router`]: `pub(super) fn router() -> Router<Arc<AppState>>`; supplies the
//!   page and asset routes for [`super::AppState`].

use super::AppState;
use axum::{
    Router,
    body::Body,
    http::{Method, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;

fn versioned_html(template: &str) -> Html<String> {
    Html(build_html(
        template,
        env!("PROCINSH_BUILD_GIT_SHA"),
        env!("PROCINSH_BUILD_GIT_DIRTY") == "true",
    ))
}

fn build_html(template: &str, sha: &str, dirty: bool) -> String {
    // The build script accepts only full hexadecimal SHAs before embedding them.
    let revision = if sha.is_empty() {
        String::new()
    } else {
        let suffix = if dirty { "-dirty" } else { "" };
        let note = if dirty {
            " (local changes to tracked files)"
        } else {
            ""
        };
        format!(
            " · <a id=\"build-commit\" href=\"https://github.com/akawashiro/procinsh/commit/{sha}\" target=\"_blank\" rel=\"noopener noreferrer\" title=\"{sha}{note}\" aria-label=\"Build commit {sha}{note}\">{}{suffix}</a>",
            &sha[..7],
        )
    };
    template
        .replace("{{PROCINSH_VERSION}}", env!("CARGO_PKG_VERSION"))
        .replace("{{PROCINSH_BUILD_REVISION}}", &revision)
}

/// Assets are embedded in debug builds too; execution never reads `web/dist`.
#[derive(rust_embed::Embed)]
#[folder = "web/dist/"]
struct WebAssets;

fn embedded_asset(path: &str) -> Response {
    let Some(asset) = WebAssets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if path.ends_with(".html") {
        let template = std::str::from_utf8(asset.data.as_ref()).expect("Vite HTML is UTF-8");
        return versioned_html(template).into_response();
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = if mime.type_() == "text" || mime.essence_str() == "application/javascript" {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    };
    (
        [(header::CONTENT_TYPE, content_type)],
        Body::from(asset.data.into_owned()),
    )
        .into_response()
}

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(|| async { embedded_asset("list/index.html") }))
        .route("/list", get(|| async { embedded_asset("list/index.html") }))
        .route(
            "/process/{pid}",
            get(|| async { embedded_asset("process/index.html") }),
        )
        .route(
            "/space",
            get(|| async { embedded_asset("space/index.html") }),
        )
        .fallback(|method: Method, uri: Uri| async move {
            let path = uri.path().trim_start_matches('/');
            if method == Method::GET || method == Method::HEAD {
                embedded_asset(path)
            } else if WebAssets::get(path).is_some() {
                StatusCode::METHOD_NOT_ALLOWED.into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{WebAssets, build_html, embedded_asset};
    use axum::{body::to_bytes, http::StatusCode};

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    fn templates() -> Vec<String> {
        ["list", "process", "space"]
            .into_iter()
            .map(|page| {
                String::from_utf8(
                    WebAssets::get(&format!("{page}/index.html"))
                        .unwrap()
                        .data
                        .into_owned(),
                )
                .unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn every_bundled_asset_is_served_with_its_content_type_and_bytes() {
        let mut scripts = 0;
        let mut styles = 0;
        for path in WebAssets::iter() {
            let response = embedded_asset(&path);
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let content_type = response.headers()["content-type"].to_str().unwrap();
            if path.ends_with(".html") {
                assert_eq!(content_type, "text/html; charset=utf-8");
            } else {
                if path.ends_with(".js") {
                    assert!(content_type.contains("javascript"));
                    scripts += 1;
                } else if path.ends_with(".css") {
                    assert_eq!(content_type, "text/css; charset=utf-8");
                    styles += 1;
                }
                let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                assert_eq!(body.as_ref(), WebAssets::get(&path).unwrap().data.as_ref());
            }
        }
        assert!(scripts > 0 && styles > 0);
        for path in ["assets/missing.js", "../Cargo.toml", "src/list/app.ts"] {
            assert_eq!(embedded_asset(path).status(), StatusCode::NOT_FOUND);
        }
    }

    #[test]
    fn known_revision_links_to_embedded_commit_on_every_page() {
        for template in templates() {
            let html = build_html(&template, SHA, false);
            assert!(html.contains(&format!("procinsh v{}", env!("CARGO_PKG_VERSION"))));
            assert!(html.contains("href=\"/list\" id=\"brand\""));
            assert!(html.contains(&format!(
                "href=\"https://github.com/akawashiro/procinsh/commit/{SHA}\""
            )));
            assert!(html.contains(&format!("title=\"{SHA}\"")));
            assert!(html.contains(&format!("aria-label=\"Build commit {SHA}\"")));
            assert!(html.contains(">0123456</a>"));
            assert!(html.contains("target=\"_blank\" rel=\"noopener noreferrer\""));
            assert!(!html.contains("{{PROCINSH_"));
            assert!(!html.contains("-dirty"));
        }
    }

    #[test]
    fn tracked_changes_are_visible_and_gitless_builds_keep_only_version() {
        for template in templates() {
            let dirty = build_html(&template, SHA, true);
            assert!(dirty.contains(">0123456-dirty</a>"));
            assert!(dirty.contains(&format!("/commit/{SHA}")));
            assert!(dirty.contains("local changes to tracked files"));
            let gitless = build_html(&template, "", false);
            assert!(gitless.contains(&format!("procinsh v{}", env!("CARGO_PKG_VERSION"))));
            assert!(!gitless.contains("id=\"build-commit\""));
            assert!(!gitless.contains("/commit/"));
            assert!(!gitless.contains("{{PROCINSH_"));
            assert!(gitless.contains(&format!(
                "<div class=\"brand\"><a href=\"/list\" id=\"brand\">procinsh v{}</a></div>",
                env!("CARGO_PKG_VERSION")
            )));
        }
    }
}
