use anyhow::anyhow;
use axum::headers::HeaderMapExt;
use axum::{
    Extension, Router,
    extract::MatchedPath,
    http::{Request, Response},
    routing::get,
};

use collab::api::CloudflareIpCountryHeader;
use collab::{
    AppState, Config, Result, api::fetch_extensions_from_blob_store_periodically, db, env,
    executor::Executor,
};
use collab::{REVISION, ServiceMode, VERSION};
use db::Database;
use sea_orm::ConnectionTrait;
use std::{
    env::args,
    net::{SocketAddr, TcpListener},
    sync::Arc,
    time::Duration,
};
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{
    Layer, filter::EnvFilter, fmt::format::JsonFields, util::SubscriberInitExt,
};
use util::ResultExt as _;

#[expect(clippy::result_large_err)]
#[tokio::main]
async fn main() -> Result<()> {
    if let Err(error) = env::load_dotenv() {
        eprintln!(
            "error loading .env.toml (this is expected in production): {}",
            error
        );
    }

    let mut args = args().skip(1);
    match args.next().as_deref() {
        Some("version") => {
            println!("collab v{} ({})", VERSION, REVISION.unwrap_or("unknown"));
        }
        Some("serve") => {
            let mode = match args.next().as_deref() {
                Some("collab") => ServiceMode::Collab,
                Some("api") => ServiceMode::Api,
                Some("all") => ServiceMode::All,
                _ => {
                    return Err(anyhow!("usage: collab <version | serve <api|collab|all>>"))?;
                }
            };

            let config = envy::from_env::<Config>().expect("error loading config");
            init_tracing(&config);
            init_panic_hook();

            let mut app = Router::new()
                .route("/", get(handle_root))
                .route("/healthz", get(handle_liveness_probe))
                .layer(Extension(mode));

            let listener = TcpListener::bind(format!("0.0.0.0:{}", config.http_port))
                .expect("failed to bind TCP listener");

            let mut on_shutdown = None;

            if mode.is_collab() || mode.is_api() {
                setup_app_database(&config).await?;

                let state = AppState::new(config, Executor::Production).await?;

                if mode.is_collab() {
                    let epoch = state
                        .db
                        .create_server(&state.config.zed_environment)
                        .await?;
                    let rpc_server = collab::rpc::Server::new(epoch, state.clone());
                    rpc_server.start().await?;

                    app = app.merge(collab::rpc::routes(rpc_server.clone()));

                    on_shutdown = Some(Box::new(move || rpc_server.teardown()));
                }

                if mode.is_api() {
                    fetch_extensions_from_blob_store_periodically(state.clone());

                    app = app
                        .merge(collab::api::events::router())
                        .merge(collab::api::extensions::router())
                }

                app = app.layer(Extension(state.clone()));
            }

            app = app.layer(
                TraceLayer::new_for_http()
                    .make_span_with(|request: &Request<_>| {
                        let matched_path = request
                            .extensions()
                            .get::<MatchedPath>()
                            .map(MatchedPath::as_str);

                        let geoip_country_code = request
                            .headers()
                            .typed_get::<CloudflareIpCountryHeader>()
                            .map(|header| header.to_string());

                        tracing::info_span!(
                            "http_request",
                            method = ?request.method(),
                            matched_path,
                            geoip_country_code,
                            user_id = tracing::field::Empty,
                            login = tracing::field::Empty,
                            authn.jti = tracing::field::Empty,
                            is_staff = tracing::field::Empty
                        )
                    })
                    .on_response(
                        |response: &Response<_>, latency: Duration, _: &tracing::Span| {
                            let duration_ms = latency.as_micros() as f64 / 1000.;
                            tracing::info!(
                                duration_ms,
                                status = response.status().as_u16(),
                                "finished processing request"
                            );
                        },
                    ),
            );

            #[cfg(unix)]
            let signal = async move {
                let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())
                    .expect("failed to listen for interrupt signal");
                let mut sigint = tokio::signal::unix::signal(SignalKind::interrupt())
                    .expect("failed to listen for interrupt signal");
                let sigterm = sigterm.recv();
                let sigint = sigint.recv();
                futures::pin_mut!(sigterm, sigint);
                futures::future::select(sigterm, sigint).await;
            };

            #[cfg(windows)]
            let signal = async move {
                // todo(windows):
                // `ctrl_close` does not work well, because tokio's signal handler always returns soon,
                // but system terminates the application soon after returning CTRL+CLOSE handler.
                // So we should implement blocking handler to treat CTRL+CLOSE signal.
                let mut ctrl_break = tokio::signal::windows::ctrl_break()
                    .expect("failed to listen for interrupt signal");
                let mut ctrl_c = tokio::signal::windows::ctrl_c()
                    .expect("failed to listen for interrupt signal");
                let ctrl_break = ctrl_break.recv();
                let ctrl_c = ctrl_c.recv();
                futures::pin_mut!(ctrl_break, ctrl_c);
                futures::future::select(ctrl_break, ctrl_c).await;
            };

            axum::Server::from_tcp(listener)
                .map_err(|e| anyhow!(e))?
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async move {
                    signal.await;
                    tracing::info!("Received interrupt signal");

                    if let Some(on_shutdown) = on_shutdown {
                        on_shutdown();
                    }
                })
                .await
                .map_err(|e| anyhow!(e))?;
        }
        _ => {
            Err(anyhow!(
                "usage: collab <version | migrate | seed | serve <api|collab|llm|all>>"
            ))?;
        }
    }
    Ok(())
}

async fn setup_app_database(config: &Config) -> Result<()> {
    let db_options = db::ConnectOptions::new(config.database_url.clone());
    let mut db = Database::new(db_options).await?;

    // zed-kask: for local SQLite dev, the database file starts empty. Apply
    // the SQLite schema migration so `initialize_notification_kinds` has
    // tables to work with. In production (Postgres), the schema is applied
    // out-of-band and the migration SQL is Postgres-specific, so this only
    // runs for SQLite backends.
    if db.pool.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
        // zed-kask: only apply the bootstrap schema when the local SQLite
        // database is empty. The migration SQL uses `CREATE TABLE "users"`
        // (not `IF NOT EXISTS`), so re-applying it on every startup crashes
        // the second run with "table users already exists". `users` is the
        // first table the bootstrap SQL creates, so its presence indicates
        // the schema was already bootstrapped on a prior run.
        let already_bootstrapped = db
            .pool
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'users' LIMIT 1",
            ))
            .await?
            .is_some();
        if !already_bootstrapped {
            let migration_sql = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/migrations.sqlite/20221109000000_test_schema.sql"
            ));
            db.pool
                .execute(sea_orm::Statement::from_string(
                    sea_orm::DatabaseBackend::Sqlite,
                    migration_sql,
                ))
                .await?;
        }
    }

    db.initialize_notification_kinds().await?;

    Ok(())
}

async fn handle_root(Extension(mode): Extension<ServiceMode>) -> String {
    format!("zed:{mode} v{VERSION} ({})", REVISION.unwrap_or("unknown"))
}

async fn handle_liveness_probe(app_state: Option<Extension<Arc<AppState>>>) -> Result<String> {
    if let Some(state) = app_state {
        state.db.project_count_excluding_admins().await?;
    }

    Ok("ok".to_string())
}

pub fn init_tracing(config: &Config) -> Option<()> {
    use std::str::FromStr;
    use tracing_subscriber::layer::SubscriberExt;

    let filter = EnvFilter::from_str(config.rust_log.as_deref()?).log_err()?;

    tracing_subscriber::registry()
        .with(if config.log_json.unwrap_or(false) {
            Box::new(
                tracing_subscriber::fmt::layer()
                    .fmt_fields(JsonFields::default())
                    .event_format(
                        tracing_subscriber::fmt::format()
                            .json()
                            .flatten_event(true)
                            .with_span_list(false),
                    )
                    .with_filter(filter),
            ) as Box<dyn Layer<_> + Send + Sync>
        } else {
            Box::new(
                tracing_subscriber::fmt::layer()
                    .event_format(tracing_subscriber::fmt::format().pretty())
                    .with_filter(filter),
            )
        })
        .init();

    None
}

fn init_panic_hook() {
    std::panic::set_hook(Box::new(move |panic_info| {
        let panic_message = match panic_info.payload().downcast_ref::<&'static str>() {
            Some(message) => *message,
            None => match panic_info.payload().downcast_ref::<String>() {
                Some(message) => message.as_str(),
                None => "Box<Any>",
            },
        };
        let backtrace = std::backtrace::Backtrace::force_capture();
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()));
        tracing::error!(panic = true, ?location, %panic_message, %backtrace, "Server Panic");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_test_config(database_url: String) -> Config {
        Config {
            http_port: 0,
            database_url,
            database_max_connections: 5,
            livekit_server: None,
            livekit_key: None,
            livekit_secret: None,
            rust_log: None,
            log_json: None,
            blob_store_url: None,
            blob_store_region: None,
            blob_store_access_key: None,
            blob_store_secret_key: None,
            blob_store_bucket: None,
            kinesis_region: None,
            kinesis_stream: None,
            kinesis_access_key: None,
            kinesis_secret_key: None,
            zed_environment: "test".into(),
            zed_cloud_internal_api_key: String::new(),
            zed_client_checksum_seed: None,
        }
    }

    // zed-kask: pins the SQLite bootstrap idempotence guard in
    // `setup_app_database`. The bootstrap SQL uses `CREATE TABLE "users"`
    // (no `IF NOT EXISTS`), so without the guard a second `collab serve`
    // against the same DB file crashes with "table users already exists".
    // The guard skips re-application when the `users` table — the first
    // table the bootstrap SQL creates — is already present.
    #[tokio::test]
    async fn setup_app_database_is_idempotent_for_sqlite() {
        let db_path = std::env::temp_dir().join(format!(
            "collab-bootstrap-idempotence-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let database_url = format!("sqlite://{}?mode=rwc", db_path.display());
        let config = sqlite_test_config(database_url.clone());

        // First run against an empty file: applies the bootstrap schema.
        setup_app_database(&config)
            .await
            .expect("first bootstrap must succeed");

        // Second run against the same file: the guard must skip the
        // bootstrap SQL instead of crashing on `CREATE TABLE "users"`.
        setup_app_database(&config)
            .await
            .expect("second bootstrap against the same DB must succeed");

        // The `users` table must exist exactly once.
        let db_options = db::ConnectOptions::new(database_url);
        let db = Database::new(db_options)
            .await
            .expect("must reopen the bootstrapped DB");
        let row = db
            .pool
            .query_one(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS users_table_count FROM sqlite_master \
                 WHERE type = 'table' AND name = 'users'",
            ))
            .await
            .expect("sqlite_master query must succeed")
            .expect("COUNT must return a row");
        let users_table_count: i64 = row
            .try_get("", "users_table_count")
            .expect("users_table_count column must be readable");
        assert_eq!(
            users_table_count, 1,
            "the users table must exist exactly once after two bootstraps"
        );

        std::fs::remove_file(&db_path).log_err();
        std::fs::remove_file(db_path.with_extension("db-wal")).log_err();
        std::fs::remove_file(db_path.with_extension("db-shm")).log_err();
    }
}
