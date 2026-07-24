//! Onboarding page — served at /onboarding after invite acceptance or first sign-in.
//!
//! expect: "As a new member I am guided through the onboarding process"
//! Introduces the userpod, Matrix credentials, the energy model, and next steps.

use axum::{extract::Query, response::Html};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OnboardingQuery {
    pub name: Option<String>,
    pub userpod: Option<String>,
}

/// GET /onboarding?name=Alice+Smith&userpod=alice-smith
///
/// Fills in Matrix credentials and userpod name from query params.
/// Without query params, shows the generic version with placeholders.
pub async fn onboarding_page(Query(q): Query<OnboardingQuery>) -> Html<String> {
    let userpod_name = q.userpod.as_deref().unwrap_or("your-userpod");
    let display_name = q.name.as_deref().unwrap_or("there");

    // Derive Matrix domain from homeserver URL
    let matrix_domain = std::env::var("HKASK_MATRIX_URL")
        .ok()
        .and_then(|url| {
            url.trim_start_matches("http://")
                .trim_start_matches("https://")
                .split(':')
                .next()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "localhost".to_string());

    // Derive Matrix usernames from shared onboarding service formula
    let (human_localpart, userpod_localpart) =
        hkask_services_onboarding::derive_matrix_localparts(display_name, userpod_name);
    let human_matrix = format!("@{human_localpart}:{matrix_domain}");
    let userpod_matrix = format!("@{userpod_localpart}:{matrix_domain}");

    let html = ONBOARDING_HTML
        .replace("USERPOD_NAME", userpod_name)
        .replace("HUMAN_MATRIX_ID", &human_matrix)
        .replace("USERPOD_MATRIX_ID", &userpod_matrix)
        .replace("MATRIX_DOMAIN", &matrix_domain);

    Html(html)
}

const ONBOARDING_HTML: &str = r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>hKask — Welcome</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: #0d1117;
    color: #e6edf3;
    padding: 20px;
  }
  .container { max-width: 640px; margin: 0 auto; }
  .card {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 36px;
    margin-bottom: 20px;
  }
  h1 { font-size: 1.5rem; margin-bottom: 4px; color: #58a6ff; }
  h2 { font-size: 1.1rem; margin-bottom: 12px; color: #7ee787; }
  .subtitle { color: #8b949e; margin-bottom: 24px; font-size: 0.95rem; }
  .section { margin-bottom: 20px; }
  .section h3 { color: #d2a8ff; margin-bottom: 8px; font-size: 0.95rem; }
  .section p { color: #8b949e; font-size: 0.9rem; line-height: 1.5; margin-bottom: 6px; }
  .code-block {
    background: #0d1117;
    border: 1px solid #30363d;
    border-radius: 6px;
    padding: 12px 16px;
    font-family: Menlo, Monaco, monospace;
    font-size: 0.85rem;
    color: #e6edf3;
    margin: 8px 0;
    word-break: break-all;
  }
  .code-block .label { color: #8b949e; }
  .code-block .value { color: #7ee787; }
  .divider { border: none; border-top: 1px solid #30363d; margin: 20px 0; }
  .btn {
    display: block;
    background: #238636;
    color: #fff;
    padding: 14px 32px;
    border-radius: 8px;
    text-decoration: none;
    font-weight: 600;
    font-size: 1rem;
    text-align: center;
    margin-bottom: 10px;
    border: none;
    cursor: pointer;
  }
  .btn:hover { background: #2ea043; }
  .btn-outline {
    display: block;
    background: transparent;
    color: #58a6ff;
    padding: 10px 32px;
    border-radius: 8px;
    text-decoration: none;
    font-size: 0.9rem;
    text-align: center;
    border: 1px solid #30363d;
  }
  .btn-outline:hover { background: #1c2128; }
  .highlight { color: #d2a8ff; font-weight: 600; }
  .emoji { font-size: 1.2rem; }
</style>
</head>
<body>
<div class="container">

  <!-- Identity Card -->
  <div class="card" style="text-align:center;">
    <h1><span class="emoji">&#x2130;</span> Welcome to hKask</h1>
    <p class="subtitle">Your userpod is alive. This is your agent — a sovereign digital presence that
    works for you, with its own identity, capabilities, and energy budget.</p>
  </div>

  <!-- Your UserPod -->
  <div class="card">
    <h2>Your UserPod</h2>
    <div class="section">
      <h3>What is a userpod?</h3>
      <p>A userpod is an AI agent that <span class="highlight">represents you</span> in the hKask system.
      It has its own WebID, can be delegated capabilities, runs inference, creates pods,
      and communicates with other userpods through Matrix chat rooms.</p>
      <p>Think of it as your always-on assistant — it can research, analyze, generate, and
      coordinate on your behalf. You control what it can do through capability tokens and
      sovereignty boundaries.</p>
    </div>
    <div class="section">
      <h3>Your identity</h3>
      <div class="code-block">
        <span class="label">UserPod: </span><span class="value">USERPOD_NAME</span>
      </div>
      <p style="font-size:0.8rem;">Your userpod name is how the system and other users will know you.</p>
    </div>
  </div>

  <!-- Matrix Chat -->
  <div class="card">
    <h2>Team Chat (Matrix)</h2>
    <div class="section">
      <h3>Your Matrix accounts</h3>
      <p>hKask uses <span class="highlight">Matrix</span> (an open, federated chat protocol) running on
      a Conduit homeserver. Two accounts were created for you:</p>
      <div class="code-block">
        <span class="label">You:     </span><span class="value">HUMAN_MATRIX_ID</span><br>
        <span class="label">UserPod: </span><span class="value">USERPOD_MATRIX_ID</span>
      </div>
      <p style="font-size:0.8rem;">Use any Matrix client (Element, Hydrogen, FluffyChat) and connect to <strong>MATRIX_DOMAIN</strong>.
      Your userpod can also send and receive messages on your behalf.</p>
    </div>
  </div>

  <!-- Energy Economy -->
  <div class="card">
    <h2><span class="emoji">⚡</span> Energy Economy</h2>
    <div class="section">
      <h3>Your userpod runs on energy</h3>
      <p>Every inference call, every tool execution, every pod deployment consumes
      <span class="highlight">rJoules</span> — hKask's unit of computational energy.
      Just like putting gas in a car, you load energy into your wallet to power your userpod.</p>
      <p>Different operations cost different amounts. A quick chat response costs less than
      training a LoRA adapter or running a multi-hour research task. You set budgets and
      limits — your userpod won't spend energy you haven't approved.</p>
    </div>
    <div class="section">
      <h3>Getting energy</h3>
      <p>Use the terminal to check your balance and load energy:</p>
      <div class="code-block">
        <span class="label">$ </span>kask wallet balance<br>
        <span class="label">$ </span>kask wallet deposit
      </div>
    </div>
  </div>

  <!-- Next Steps -->
  <div class="card">
    <h2>Getting Started</h2>
    <div class="section">
      <h3>Your first steps</h3>
      <p><strong>1.</strong> Open the terminal and say hello to your userpod.</p>
      <p><strong>2.</strong> Try <code>kask help</code> to see all available commands.</p>
      <p><strong>3.</strong> Create a pod: <code>kask pod create --template research</code></p>
      <p><strong>4.</strong> Load energy into your wallet to enable inference.</p>
      <p><strong>5.</strong> Join the team chat and introduce yourself.</p>
    </div>
    <hr class="divider">
    <a href="/terminal" class="btn">Open Terminal</a>
    <a href="/api/v1/auth/session" class="btn-outline">View Your Session</a>
  </div>

</div>
</body>
</html>"###;

/// GET /invite-required — shown when a closed server rejects uninvited OAuth sign-in.
pub async fn invite_required_page() -> impl axum::response::IntoResponse {
    axum::response::Html(INVITE_REQUIRED_HTML)
}

const INVITE_REQUIRED_HTML: &str = r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>hKask — Invite Required</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: #0d1117;
    color: #e6edf3;
    display: flex; justify-content: center; align-items: center;
    min-height: 100vh; padding: 20px;
  }
  .card {
    background: #161b22;
    border: 1px solid #30363d;
    border-radius: 12px;
    padding: 48px;
    max-width: 480px;
    width: 100%;
    text-align: center;
  }
  h1 { font-size: 1.5rem; margin-bottom: 8px; color: #f85149; }
  .subtitle { color: #8b949e; margin-bottom: 24px; font-size: 0.95rem; }
  p { color: #8b949e; font-size: 0.9rem; line-height: 1.5; }
</style>
</head>
<body>
<div class="card">
  <h1>Invite Required</h1>
  <p class="subtitle">This hKask server is invite-only.</p>
  <p>To join, request an invite code from the server administrator.
  If you already have a code, visit:</p>
  <p style="margin-top:12px;"><code style="background:#21262d;padding:4px 8px;border-radius:4px;">/api/v1/auth/accept-invite?code=YOUR_CODE</code></p>
</div>
</body>
</html>"###;
