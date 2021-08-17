mod ifaces;
mod dns;

#[tokio::main]
async fn main() -> Result<(), String> {
    //ifaces::start().await;

    let mut addr_updates = ifaces::start();

    // dns::query().await?;
    // dns::update().await?;

    while let Some((iface, addr)) = addr_updates.recv().await {
        println!("{}: {}", iface, addr);
    }

    Ok(())
}
