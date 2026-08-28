fn main() -> Result<(), Box<dyn std::error::Error>> {
    for monitor in screendelta::monitors()? {
        println!(
            "{}: {} at {:?}",
            monitor.id.as_str(),
            monitor.name,
            monitor.region
        );
    }
    Ok(())
}
