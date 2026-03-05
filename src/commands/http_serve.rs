use anyhow::Result;
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::services::ServeDir;

pub async fn run(port: u16, dir: &str) -> Result<()> {
    let path = PathBuf::from(dir);

    let app = Router::new().fallback_service(ServeDir::new(path));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
