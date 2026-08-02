use anyhow::Result;

pub fn export_csv(path: &str, data: &str) -> Result<()> {
    std::fs::write(path, data)?;
    Ok(())
}
