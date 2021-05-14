mod ifaces;

#[tokio::main]
async fn main() -> Result<(), String> {
    ifaces::start().await;

    Ok(())
}
