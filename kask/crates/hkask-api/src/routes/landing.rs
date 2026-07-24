//! Landing page route — serves the hKask welcome page at /.
//!
//! REQ: P3-deploy-landing-page — P3 Headless: single static HTML landing page with OAuth sign-in.
//! expect: "I see a single static landing page with OAuth sign-in options"
//!
//! Also serves as the Kubernetes liveness probe — returns immediately (static HTML)
//! to prove the HTTP server is alive. The readiness probe uses `/health` instead.

use axum::response::IntoResponse;

/// GET / — landing page with logo and sign-in buttons.
/// Kept static for fast k8s liveness probe response.
#[utoipa::path(
    get,
    path = "/",
    tag = "landing",
    responses(
        (status = 200, description = "Landing page HTML", content_type = "text/html"),
    ),
)]
pub async fn landing_page() -> impl IntoResponse {
    axum::response::Html(LANDING_HTML)
}

const LANDING_HTML: &str = r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>hKask — Agent Container</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: #0d1117;
    color: #e6edf3;
    display: flex; align-items: center; justify-content: center;
    min-height: 100vh;
  }
  .container { text-align: center; max-width: 420px; padding: 2rem; }
  .logo { width: 120px; height: auto; margin-bottom: 1.5rem; opacity: 0.9; }
  h1 { font-size: 1.5rem; font-weight: 600; margin-bottom: 0.5rem; letter-spacing: 0.5px; }
  .subtitle { font-size: 0.9rem; color: #8b949e; margin-bottom: 2rem; line-height: 1.5; }
  .btn {
    display: block; width: 100%; padding: 12px 20px; margin-bottom: 10px;
    border: 1px solid #30363d; border-radius: 6px; background: #21262d;
    color: #c9d1d9; font-size: 0.95rem; cursor: pointer; text-decoration: none;
    transition: background 0.15s, border-color 0.15s;
  }
  .btn:hover { background: #30363d; border-color: #58a6ff; }
  .btn-github { background: #238636; border-color: #238636; color: #fff; }
  .btn-github:hover { background: #2ea043; border-color: #2ea043; }
  .btn-google { background: #21262d; border-color: #30363d; }
  .footer { margin-top: 2rem; font-size: 0.75rem; color: #484f58; }
  .footer a { color: #58a6ff; text-decoration: none; }
</style>
</head>
<body>
<div class="container">
  <svg class="logo" viewBox="0 0 400 600" xmlns="http://www.w3.org/2000/svg">
    <title>Kask</title>
    <rect width="400" height="600" fill="none"/>
    <g opacity="0.12">
      <rect x="120" y="185" width="140" height="255" rx="12" fill="none" stroke="#e6edf3" stroke-width="5"/>
      <rect x="150" y="145" width="80" height="45" fill="none" stroke="#e6edf3" stroke-width="5"/>
      <path d="M 120 200 C 95 200 95 255 120 265" fill="none" stroke="#e6edf3" stroke-width="4" stroke-linecap="round"/>
      <path d="M 260 200 C 285 200 285 255 260 265" fill="none" stroke="#e6edf3" stroke-width="4" stroke-linecap="round"/>
    </g>
    <path d="M 145 185 L 145 440" fill="none" stroke="#e6edf3" stroke-width="9" stroke-linecap="round"/>
    <path d="M 285 185 L 285 440" fill="none" stroke="#e6edf3" stroke-width="9" stroke-linecap="round"/>
    <path d="M 145 440 Q 215 445 285 440" fill="none" stroke="#e6edf3" stroke-width="9" stroke-linecap="round"/>
    <path d="M 145 185 Q 215 180 285 185" fill="none" stroke="#e6edf3" stroke-width="7" stroke-linecap="round"/>
    <path d="M 170 145 L 170 185" fill="none" stroke="#e6edf3" stroke-width="8" stroke-linecap="round"/>
    <path d="M 260 145 L 260 185" fill="none" stroke="#e6edf3" stroke-width="8" stroke-linecap="round"/>
    <path d="M 170 145 Q 215 140 260 145" fill="none" stroke="#e6edf3" stroke-width="7" stroke-linecap="round"/>
    <path d="M 165 145 C 165 138 215 135 265 145 C 265 152 215 155 165 145" fill="none" stroke="#e6edf3" stroke-width="6" stroke-linecap="round"/>
    <path d="M 145 205 C 115 205 110 235 120 255 C 128 265 138 268 145 265" fill="none" stroke="#e6edf3" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M 285 205 C 315 205 320 235 310 255 C 302 265 292 268 285 265" fill="none" stroke="#e6edf3" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>
    <path d="M 175 310 Q 215 295 255 310" fill="none" stroke="#e6edf3" stroke-width="8" stroke-linecap="round"/>
    <path d="M 178 330 Q 215 350 252 330" fill="none" stroke="#e6edf3" stroke-width="5" stroke-linecap="round"/>
    <circle cx="215" cy="320" r="18" fill="#e6edf3"/>
    <circle cx="215" cy="320" r="10" fill="#0d1117"/>
    <circle cx="220" cy="315" r="5" fill="#e6edf3"/>
    <path d="M 188 308 L 185 302" stroke="#e6edf3" stroke-width="2" stroke-linecap="round"/>
    <path d="M 202 305 L 200 298" stroke="#e6edf3" stroke-width="2" stroke-linecap="round"/>
    <path d="M 228 305 L 230 298" stroke="#e6edf3" stroke-width="2" stroke-linecap="round"/>
    <path d="M 242 308 L 245 302" stroke="#e6edf3" stroke-width="2" stroke-linecap="round"/>
    <text x="200" y="540" font-family="monospace" font-size="18" fill="#e6edf3" text-anchor="middle" letter-spacing="8">KASK</text>
  </svg>
  <h1>hKask</h1>
  <p class="subtitle">A Minimal Viable Container for UserPods.<br>Sign in to access your terminal and sovereign agent workspace.</p>
  <a href="/api/v1/auth/login?provider=github" class="btn btn-github">Sign in with GitHub</a>
  <p class="footer">No client install required. Just a browser.<br><a href="https://github.com/mdz-axo/hKask">github.com/mdz-axo/hKask</a></p>
</div>
</body>
</html>"###;
