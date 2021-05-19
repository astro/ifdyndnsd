//mod ifaces;
mod dns;

#[tokio::main]
async fn main() -> Result<(), String> {
    //ifaces::start().await;

    dns::query().await?;
    dns::update().await?;

    Ok(())
}
