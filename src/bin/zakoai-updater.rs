#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();
    let state = zakoai_proxy::update::Agent::from_env().await?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9000").await?;
    axum::serve(listener, zakoai_proxy::update::agent_app(state)).await?;
    Ok(())
}
